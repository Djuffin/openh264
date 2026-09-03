> **Recovered from `eb463dbd^` on 2026-08-07** (Phase 0 of the safety refactor).
> `eb463dbd` deleted this file; it is kept as the historical per-phase log of the
> encoder port — how each defect was found, which is the part that does not go stale.
> **Every claim in it is unverified at HEAD**, and its phase log stops before
> `fa67432f`, `909c368b`, `353791f7` and the Phase 0 commits. The current strategy
> document is [`safety_refactor_plan.md`](safety_refactor_plan.md); the running
> record is [`safety_refactor_log.md`](safety_refactor_log.md).

# Encoder port status

Living record of the OpenH264 **encoder** Rust port. The decoder port is mature and
passes conformance; the encoder is the work in progress. Update this file at the end
of every phase.

> **Starting a session? Read [`encoder_port_handoff.md`](encoder_port_handoff.md)
> first.** It is the brief: current state with measured numbers, the three techniques
> that work, the five defect classes, the remaining work in order, and the traps.
> This file is the *record* — the per-phase log and every defect found so far, with
> how it was found. The handoff tells you what to do; this file tells you what
> happened.

**As of Phase 5.3 the encoder is byte-identical with the C++ reference for all five
rate-control modes, both entropy coders, both init paths, all 52 QPs, and **all four
slice modes**.** Verified over 2632 configurations. `SetOption` handles all 32 of
upstream's options; `todo!()` and `unimplemented!()` are both zero in `src/`.
`cargo test --no-fail-fast` is green: 293 passed, 0 failed, 20 ignored, and
`test_decode_encode_full_cycle_sha1_parity` — upstream's own hash — passes.

**The one axis still pinned is `iMultipleThreadIdc == 1`.** After that, one spatial
layer (which needs `METHOD_DOWNSAMPLE`) and `SCREEN_CONTENT_REAL_TIME`. See
*Phase 5.4*. *(2026-09-02: multi-threading and multi-layer have long since landed;
`SCREEN_CONTENT_REAL_TIME` is Phase 10's, opened by P10.1 and with its three
video-processing plugins in since P10.2 — see the Phase 10
section at the end of this file.)*

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

`compare.sh <yuv> <w> <h> <frames> <qp> <cabac> <gop> [rcmode] [baseinit] [slicemode]
[slicenum] [threads]` runs both encoders and `cmp`s the Annex-B output; it exits 0
only when the streams match.
`rcmode` defaults to `RC_OFF_MODE`; `baseinit=1` selects `Initialize(SEncParamBase)`;
`slicemode` is 0 `SM_SINGLE_SLICE` (default), 1 `SM_FIXEDSLCNUM_SLICE`,
2 `SM_RASTER_SLICE`, 3 `SM_SIZELIMITED_SLICE`, with `slicenum` the slice count for
1 and 2, the rows-per-slice for 2, and the byte constraint for 3; `threads` is
`iMultipleThreadIdc`.
It also feeds the Rust stream to `./h264dec` as a sanity check. Artifacts land in
`rust/tools/diffharness/out/` (gitignored).

> **Trap: the drivers take `out.264` BEFORE `rcmode`.** `cxx_enc … -1 0 out.264`
> silently encodes on `RC_QUALITY_MODE`, because `atoi` of a file path is 0. If a
> dump looks like the C++ is ignoring a parameter, check the argument order first.

> **Trap: sweep scripts must quote their parameter expansions.** The shell here is
> zsh, which does *not* word-split unquoted expansions, so `for spec in "a b c"; do
> set -- $spec` leaves `$2` and `$3` empty and every `compare.sh` invocation runs on
> garbage arguments. It reads as a uniform failure across the whole sweep.

> **Trap, and the expensive one: `cabac 1` was a no-op for five phases.** Both
> drivers pinned `uiProfileIdc = PRO_BASELINE`, and `ParamValidationExt`
> (`encoder_ext.cpp:655`) resets `iEntropyCodingModeFlag` to 0 for a baseline layer.
> A 120-configuration CABAC sweep passed with 8034 == 8034 and no CABAC symbol was
> ever written. The drivers now pick `PRO_HIGH` when the flag is set. **Before
> trusting a sweep over a newly-enabled feature, check that turning the knob changes
> the C++ output.**

### Why not drive `h264enc` directly

`h264enc` derives most of `SEncParamExt` from `FillSpecificParameters()` plus config
files, so matching it from Rust means replicating CLI parsing rather than testing the
encoder. Instead `cxx_enc.cpp` and `rust_enc/main.rs` each set a **fully explicit,
identical** `SEncParamExt`. The only variable under test is the encoder itself.

One trap worth recording: `h264enc` silently drops frames when the input frame rate
and the layer output frame rate disagree — `-frms 5` on a 5-frame file yields
`Frames: 2` unless you also pass `-frin 6 -frout 0 6`. The harness sidesteps this by
setting `fMaxFrameRate` and `sSpatialLayers[0].fFrameRate` to the same value.

### Gate configuration (Phase 5 target — **met, and long since exceeded**)

Single spatial layer, single slice, CAVLC, RC off, single-threaded, deblocking on,
`CONSTANT_ID`, no LTR/denoise/AQ/BGD/scene-change. See `cxx_enc.cpp` for the exact
field-by-field setting. As of Phase 5.2 the measured range is far wider: all five
`iRCMode` values, both entropy coders, both init paths, all 52 QPs, and
`SM_FIXEDSLCNUM_SLICE`/`SM_RASTER_SLICE` at 2/3/4/6 slices. The axes still pinned are
**one thread**, **one spatial layer** and `SM_SIZELIMITED_SLICE`.

---

## C++ oracle

| Tool | Command | Status |
|---|---|---|
| `libopenh264.a`, `h264enc`, `h264dec` | `make -j8 libraries binaries` | builds clean |
| `codec_unittest` | `make gtest-bootstrap && make -j8 test` | builds; 534 tests, see below |

`codec_unittest` baseline on this machine (darwin/arm64): **533 pass, 1 fail**
(re-measured at Gate 4.6). The failure is `DecoderDeblocking.DeblockingInit`,
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
- **E.** *(fixed, Phases 4.5 and 4.6)* The **mode-decision layer was unported** — 22
  of the 32 functions in `svc_base_layer_md.cpp` — and so were three whole files it
  depends on, plus twelve `mv_pred.cpp` helpers and, as Phase 4.6 found, the whole
  `codec/processing/` VP module. Both halves are now ported. See *Phase 4.5* and
  *Phase 4.6*.

**The encoder is byte-identical with the C++ reference across the whole gate
configuration.** Verified on four inputs (160x96, 320x192, 152x100, 1280x720), all
52 QPs but one, and GOP sizes 1/2/4/8. Rate control off is exact; the QP-adapting
rate-control modes are not yet — see *Phase 5*.

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
| `rust/tools/find_stub_bodies.py` | Phase 5.1 — call-set audit against the C++ body; `--dups` lists duplicated Rust definitions worst-disparity-first |
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

### Phase 4.6 — the inter mode-decision path — **DONE, byte-exact**

**Both Gate B and Gate B+ are met.** Measured, not asserted:

```
./rust/tools/diffharness/compare.sh res/CiscoVT2people_160x96_6fps.yuv 160 96 5 26 0 -1
  C++  : 8034 bytes
  Rust : 8034 bytes
  RESULT: BYTE-IDENTICAL
  Rust stream decodes to 115200 bytes YUV      (5 frames, was 1)
```

| check | result |
|---|---|
| `cargo build` | clean, 15 warnings, all `dead_code`/`unused` |
| `cargo test --no-fail-fast` | **289 passed, 0 failed, 21 ignored** (was 287/2/20) |
| decoder conformance | 53/53 |
| `codec_unittest` | 533/534, only `DecoderDeblocking.DeblockingInit` |
| `find_dup_types.sh` | silent on both passes |
| `todo!()` in `src/` | **0**; one `unimplemented!()`, the CABAC branch of `WelsWriteSliceEndSyn` |

The suite is green for the first time. Two failures were closed and one was
`#[ignore]`d with a measurement — see *The SHA-1 parity test* below.

#### The transcription

New in `svc_base_layer_md.rs`: `WelsMdInterInit`, `WelsMdP16x8`, `WelsMdP8x16`,
`WelsMdP4x4`, `WelsMdP8x4`, `WelsMdP4x8`, `WelsMdInterFinePartition`,
`WelsMdInterFinePartitionVaa`, `WelsMdPSkipEnc`, `WelsMdInterMbRefinement`,
`WelsMdFirstIntraMode`, `WelsMdInterMb`, `WelsMdInterDoubleCheckPskip`,
`WelsMdInterEncode`, `WelsMdInterSaveSadAndRefMbType`, plus `g_kuiSmb4AddrIn256`
and both `g_kiPixStrideIdx*` tables.

New in `svc_mode_decision.rs`: `InitMe` and the twelve `mv_pred.cpp` helpers
(`UpdateP16x8MotionInfo`, `update_P8x16_motion_info` — the C++ really does spell
that one in snake case — `UpdateP8x8`/`P4x4`/`P8x4`/`P4x8MotionInfo`, and the six
`Update*Motion2Cache`).

> **`ST16`/`ST64` are transcribed as raw unaligned stores, not element-wise
> assignment.** `BUTTERFLY1x2(b)` is `((b)<<8)|(b)` on an `int8_t` promoted to
> `int`, so for a *negative* reference index the two bytes are not equal — the high
> byte picks up the sign extension. Element-wise assignment is right only for the
> non-negative indices the encoder happens to pass today.

#### Seven more functions existed under the right name and did a fraction of the work

The Phase-4.5 correction generalises: this is now the largest single defect class in
the port. Every one of these was found by the call-set comparison described under
Phase 6, then confirmed by reading:

| function | what was missing |
|---|---|
| `WelsMdInterJudgePskip` | ran `PredictSadSkip` unconditionally and **always returned false**, so no macroblock could ever be coded P_SKIP |
| `WelsMdInterUpdatePskip` | set `uiMbType` (which the C++ does *not* do here) and skipped the luma/chroma QP carry-over and `bCollocatedPredFlag` entirely |
| `WelsMdInterDecidedPskip` | never called `WelsRecPskip`, so a skipped macroblock's motion-compensated samples never reached the reconstruction |
| `WelsMdInterSecondaryModesEnc` | omitted `pfFirstIntraMode`, `pfSetScrollingMv`, `pfInterFineMd`, `WelsMdInterMbRefinement`, `WelsMdInterDoubleCheckPskip` |
| `WelsMdP16x16`, `WelsMdP8x8` | never called `InitMe`, so the search block kept the *previous* macroblock's `pEncMb`/`pRefMb`/`uiBlockSize`/`pMvdCost`. `WelsMdP8x8` also dropped the `sMvc` seed, `UpdateP8x8Motion2Cache` and the `iBlock8x8StaticIdc`-selected search function |
| `WelsPMbChromaEncode` | omitted **both** `WelsEncRecUV` calls — the same defect Phase 4.5 found in `WelsIMbChromaEncode` |
| `OutputPMbWithoutConstructCsRsNoCopy` | omitted the **entire luma half**: no `pDecY`, no `WelsIDctT4RecOnMb`. See below |

`WelsMdInterMbLoop` gained the three corrections listed in the Phase-4.6 plan, and
`WelsPSliceMdEnc` gained `bMdUsingSad`, which was never assigned — so every skip and
refinement cost came from SATD where the gate configuration (`LOW_COMPLEXITY`) costs
with SAD. `WelsCodePSlice`/`WelsCodePOverDynamicSlice` now assign `pfInterMd` per
slice; it had never been assigned at all. `PreprocessSliceCoding` fills
`pfFirstIntraMode` and both arms of `pfInterFineMd`, closing the three `// UNPORTED`
markers.

#### A fourth whole subsystem was unported: `codec/processing/`

`CWelsPreProcess::WelsPreprocessCreate` allocated a **zeroed** `IWelsVP`, so every
`Init`/`Set`/`Process`/`Get` was `None` and the wrapper returned 0 — *success* —
without writing anything. Nothing in the tree said so.

The visible consequence: `pVaa->sVaaCalcInfo.pSad8x8` stayed all-zero, so
`MdInterAnalysisVaaInfo_c` reported `uiMbSign == 15` ("all four 8x8 blocks flat")
for every macroblock, so `WelsMdInterFinePartitionVaa` returned immediately and
**no sub-16x16 inter partition was ever evaluated**.

New `src/processing/` implements `METHOD_VAA_STATISTICS` (`VAACalcSad_c` plus the
`CVAACalculation` Set/Process pair) behind a real `IWelsVP` vtable. Every other
method now returns `RET_NOTSUPPORTED` rather than silently succeeding; each is off
in the gate configuration, and each caller in `wels_preprocess.rs` already skips its
follow-up `Get` on a non-success return, so the observable behaviour is unchanged
but no longer silent. `pfVAACalcSadVar`/`SadSsd`/`SadBgd`/`SadSsdBgd` are likewise
`RET_NOTSUPPORTED`, gated on rate control, AQ and background detection.

#### The three defects that byte-exactness actually turned on

All three were found by bisecting with matched instrumentation, none by reading:

| defect | how it showed up |
|---|---|
| **`InitExpandPictureFunc` did not exist.** The encoder's `sExpandPicFunc` was never populated, so `WelsUpdateRefList`'s `ExpandReferencingPicture` found `None` in every slot and expanded nothing. The reference picture's padding border stayed **zero**, so every motion search that looked outside the frame compared against black. | The per-ME dump showed identical inputs and different results on the very first P macroblock; printing the reference row *above* the block showed `174,174,…` in C++ and `0,0,…` in Rust. Now in `common/expand_pic.rs`, called from `InitFunctionPointers` as `encoder.cpp:193` does. |
| **The VP module above.** | The `WelsMdInterFinePartitionVaa` dump showed `sign=15 sad8x8=0,0,0,0` for every macroblock. |
| **`OutputPMbWithoutConstructCsRsNoCopy` had no luma half.** Every inter macroblock's luma residual was never added back into the reconstruction. | A per-frame checksum of `pDecPic` showed **luma differing and both chroma planes matching** on the first P frame — while that frame's *bitstream* was byte-identical. An encoder whose reconstruction disagrees with its own (correct) bitstream is invisible until a second P frame references it. |

#### `g_kuiGolombUELength` had three copies and two were wrong

Found by encoding 320x192 rather than 160x96: the Rust encoder **aborted** at frame 7
with an index-out-of-bounds in `BsWriteUE`.

| copy | entries | agrees with `common_tables.cpp:886`? |
|---|---|---|
| `svc_set_mb_syn_cavlc.rs` | **253** | no — diverges from index 125 |
| `vlc_encoder.rs` | **274** | no — diverges from index 126 |
| `svc_encode_slice.rs` | 256 | yes |

The short one is the copy the macroblock-layer writer indexes, so any `ue(v)` of 253
or more indexed past the end, and every value from 125 up was written with the wrong
bit count. All three now re-export a single canonical copy in
`common/wels_common_defs.rs`, which is where the C++ declares it
(`wels_common_defs.h:79`). **This is the fifth phase in a row in which a duplicated
constant turned out to hold different values in different modules.**

#### How far byte-exactness reaches

Everything below is `compare.sh` output, not inference.

| axis | range tested | result |
|---|---|---|
| input | `CiscoVT2people_160x96_6fps`, `CiscoVT2people_320x192_12fps`, `Static_152_100` (not a multiple of 16), `Cisco_Absolute_Power_1280x720_30fps` | all byte-identical |
| QP | 0…51, every value | byte-identical except **qp=0** |
| GOP | `-1`, 1, 2, 4, 8 | all byte-identical |
| rate control | `RC_OFF_MODE`, `RC_BUFFERBASED_MODE` | byte-identical |
| rate control | `RC_QUALITY_MODE`, `RC_BITRATE_MODE`, `RC_TIMESTAMP_MODE` | differed at the time of Phase 4.6; **closed in Phase 5.1** |

**The one QP that differs is 0, and only above 160x96.** At 320x192 a *single-frame*
encode already differs, by 3 bytes inside the IDR — so this is an **I-slice** edge
case, not inter, and it reproduces at the Phase-4.6 starting commit, so it predates
this work. Phase 4.5 only ever measured one resolution at one QP, which is why it
was never seen. `res/CiscoVT2people_160x96_6fps.yuv` at qp=0 *is* byte-identical, so
whatever it is depends on picture size as well as QP.

#### The SHA-1 parity test

`loopback_sha1_test::test_decode_encode_full_cycle_sha1_parity` mirrors upstream's
`DecodeEncodeFile/DecodeEncodeTest.CompareOutput`, whose expected hash it carries.
Two things were wrong with it, one of them ours:

- It built an `SEncParamExt` by hand with a 500 kbit/s target and called
  `InitializeExt`. Upstream's `BaseEncoderTest::InitWithParam` takes its
  `bBaseParamFlag` branch for this configuration and calls `Initialize` with a zeroed
  `SEncParamBase` carrying `iTargetBitrate = 5000000`. The hash could not have
  matched whatever the encoder did. Now corrected to upstream's call.
- With that corrected, it still fails — because `SEncParamBase` leaves `iRCMode` at
  its `FillDefault` value `RC_QUALITY_MODE`, i.e. **rate control on**, and the port
  is byte-exact only with it off. `#[ignore]`d with that measurement in the doc
  comment, not with a guess.

#### `compare.sh` takes an optional rate-control mode

`compare.sh <yuv> <w> <h> <frames> <qp> <cabac> <gop> [rcmode]`. Omitted means
`RC_OFF_MODE`, the gate configuration. This is what produced the rate-control row of
the table above, and it is the tool the next phase needs.

#### The differential-bisection dumps are half-kept

The Rust half of the instrumentation is now permanent and env-gated through
`encoder::dump_enabled`, which caches the lookup in a `OnceLock` so the hot paths pay
one relaxed load:

| variable | printed at |
|---|---|
| `OH264_MBDUMP` | per macroblock in `WelsMdInterMbLoop`, after the mode decision |
| `OH264_MEDUMP` | per motion-search call, inputs and result |
| `OH264_FPDUMP` | per macroblock in `WelsMdInterFinePartitionVaa` |
| `OH264_RECDUMP` | per frame in `WelsUpdateRefList`, a checksum of each reconstructed plane |

Only the C++ half has to be re-patched next time (and `git checkout codec/`
afterwards). All three of this phase's byte-level defects fell out of these four
dumps in four `diff`s.

### Phase 5 — byte-exactness — **DONE for the gate configuration**

`compare.sh` exits 0 across the whole gate configuration (single spatial layer,
single slice, CAVLC, RC off, single-threaded, deblocking on, `CONSTANT_ID`, no
LTR/denoise/AQ/BGD/scene-change — see `cxx_enc.cpp`), on four inputs, all QPs but
one, and five GOP sizes. **Gate 5 met.** What remains is widening past that
configuration.

**Gate 5:** `compare.sh` exits 0.

**Gate 5 met.** Phase 5.1 closed items 1 and part of 4; what remains is listed as
*Phase 5.2* below.

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

### Phase 5.1 — rate control, and the rest of the VP module — **DONE, byte-exact**

**All five `iRCMode` values are byte-identical with the C++, and so is upstream's
own `Initialize(SEncParamBase)` configuration** — which turns on frame skip,
background detection and scene-change detection as well as rate control.
`test_decode_encode_full_cycle_sha1_parity` passes with upstream's expected hash;
it had been `#[ignore]`d since Phase 4.5.

Measured, not asserted. `compare.sh` exits 0 for:

| axis | range | configurations |
|---|---|---|
| `iRCMode` x input x GOP | 5 modes x 4 inputs (160x96, 320x192, 152x100, 1280x720) x GOP −1/2/8 | 60 |
| the same, through `Initialize(SEncParamBase)` | as above with the new tenth argument | 60 |
| QP x `iRCMode` | all 52 QPs x the four RC-on modes, 160x96 | 208 |

| check | result |
|---|---|
| `cargo build` | clean, 16 warnings, all `dead_code`/`unused` |
| `cargo test --no-fail-fast` | **290 passed, 0 failed, 20 ignored** (was 289/0/21) |
| decoder conformance | 53/53 |
| `codec_unittest` | 533/534, only `DecoderDeblocking.DeblockingInit` |
| `find_dup_types.sh` | silent on both passes |
| `todo!()` in `src/` | **0**; one `unimplemented!()`, the CABAC branch of `WelsWriteSliceEndSyn` |

#### The Phase-5 plan named rate control; four of the eight defects were elsewhere

Every one was found by bisecting the bitstream against an instrumented C++ build.
None was visible by reading, and the call-set audit over `rc.rs` — which the
Phase-5 plan reported as clean, correctly — could not have seen any of them.

| defect | detail |
|---|---|
| **`GomRCInitForOneSlice` had no call site.** | The function was ported and faithful; `WelsCodeOneSlice` simply never called it (`svc_encode_slice.cpp:1665`), so `iTargetBitsSlice` stayed 0 for every slice and the entire GOM bit-allocation path ran on a zero budget. **This is the stub-under-a-correct-name class one level up: the callee was right and the call was missing.** An audit that compares function *bodies* cannot see it; only diffing the caller against the C++ can. |
| **`METHOD_COMPLEXITY_ANALYSIS` was unported** (`RET_NOTSUPPORTED`), so `iFrameComplexity` stayed 0 and `pCurrentFrameGomSad` stayed all-zero — `RcGomTargetBits` fell to its even-split arm on every GOM and `RcCalculatePictureQp`'s complexity ratio was meaningless. Now `processing/complexity_analysis.rs`. |
| **`wels_preprocess.rs` declared `GOM_SAD = 1, GOM_VAR = 2`.** `IWelsVP.h:215` says **−1 and −2**. Both GOM modes fell through `CComplexityAnalysis::Process`'s `default:` arm, so even after the plugin existed it did nothing. Sixth phase running in which a duplicated constant held a wrong value. |
| **`VAACalcSadVar_c` was unported**, so `pSum16x16`/`pSumOfSquare16x16` were never filled. `AnalyzeSpatialPic` selects that kernel for every I slice once `iRCMode >= RC_BITRATE_MODE` (`wels_preprocess.cpp:283`). |
| **`RcCalculateGomQp` sorted its thresholds "correctly".** C++ tests `> 10600` *before* `> 11900`, which makes the `-= 2` arm unreachable. Sorting them drops the QP by 2 wherever the reference drops it by 1. The C++'s dead branch is load-bearing precisely because it is dead. |
| **`ParamTranscode` hardcoded five fields** (`bFixRCOverShoot`, `iIdrBitrateRatio`, `bPsnrY/U/V`) where `param_svc.h:349-353` copies them from the caller — a `FillDefault` block pasted into the transcoder. `bFixRCOverShoot` was true for every caller, which sends `RcInitVGop` down its carry-over arm, so `iRemainingBits` was wrong from the ninth frame on. |
| **`WelsMdUpdateBGDInfo` was shadowed by an empty stub.** `encoder_context.rs` declared its own two-line no-op with the same name, and `WelsInitBGDFunc` in that module resolved to it rather than to the real one in `svc_mode_decision.rs` — a local item beats an import. No background macroblock was ever converted from `MB_TYPE_BACKGROUND` to `MB_TYPE_SKIP`, so every one was coded in full instead of as a skip run. **`find_dup_types.sh` checks types, so nothing in the tree saw it.** See *A duplicate-function scan* below. |
| **`iSumSad` in `RcGomTargetBits` overflows.** It is `int32_t` in C++ and really does wrap: under `GOM_VAR` each `pCurrentFrameGomSad[j]` is a whole GOM's luma variance, order 1e9 at 720p, and the sum runs over 20+ GOMs. The debug build trapped on it (720p, `RC_TIMESTAMP_MODE`, GOP 2). Now an explicit `wrapping_add`. |

#### The rest of `codec/processing/`

`METHOD_ADAPTIVE_QUANT`, `METHOD_BACKGROUND_DETECTION` and
`METHOD_SCENE_CHANGE_DETECTION_VIDEO` are ported, as are the four remaining VAA
kernels (`VAACalcSadVar_c`, `VAACalcSadSsd_c`, `VAACalcSadBgd_c`,
`VAACalcSadSsdBgd_c`), so `CVAACalculation::Process`'s full four-way selection
now works instead of returning `RET_NOTSUPPORTED` for three of its four arms.

> **`bEnableAdaptiveQuant` is dead in upstream.** `ParamValidation`
> (`encoder_ext.cpp:301`) sets it to `false` unconditionally — "turn off adaptive
> quant now, algorithms needs to be refactored" — and the port already did the
> same, so `pMotionTextureIndexToDeltaQp` is never even allocated.
> `processing/adaptive_quantization.rs` is therefore correct-but-unreachable
> today. It is kept because the plugin table has to answer the method, and
> because the flag is a public API field.

Still `RET_NOTSUPPORTED`: `METHOD_DENOISE`, `METHOD_DOWNSAMPLE` (needed for more
than one spatial layer), `METHOD_SCENE_CHANGE_DETECTION_SCREEN`,
`METHOD_COMPLEXITY_ANALYSIS_SCREEN`, `METHOD_SCROLL_DETECTION` — the last three
all under `SCREEN_CONTENT_REAL_TIME`.

#### A duplicate-function scan, and what it found immediately

`rust/tools/find_stub_bodies.py` is new and does two things:

```bash
rust/tools/find_stub_bodies.py                    # call-set audit vs the C++ body
rust/tools/find_stub_bodies.py --dups             # duplicated Rust definitions
```

`--dups` lists every function name defined more than once, worst body-size
disparity first. It is the check that would have caught `WelsMdUpdateBGDInfo` in
seconds. Run over the tree it reported **93 duplicated function names**, and the
four worst were all the same shape — an empty or near-empty body shadowing a real
one:

| name | stub | real |
|---|---|---|
| `WelsMdUpdateBGDInfo` | `encoder_context.rs`, empty | `svc_mode_decision.rs` — **was live, see above** |
| `WelsRcInitFuncPointers` | `wels_encoder_ext.rs`, set `iRCMode` and nothing else | `rc.rs` |
| `FilterLTRRecoveryRequest`, `FilterLTRMarkingFeedback` | `wels_encoder_ext.rs`, empty | `ref_list_mgr_svc.rs` |
| `UpdateMbNeighbor` | `slice_multi_threading.rs`, empty | `svc_encode_slice.rs` |

All four stubs are deleted. `UpdateMbNeighbor`'s call site now uses the real one.
The other three had no reachable caller — see the next section for why that is
its own problem.

#### `SetOption` silently succeeds for 14 of upstream's options

Found while chasing `WelsRcInitFuncPointers`. `CWelsH264SVCEncoder::SetOption`
(`welsEncoderExt.cpp:692`) handles about 25 `ENCODER_OPTION_*` values; the port
handles 11 and ends with `_ => {}` followed by `return 0`. **Every other option
is accepted and ignored**, including `ENCODER_OPTION_RC_MODE`,
`ENCODER_OPTION_RC_FRAME_SKIP`, `ENCODER_OPTION_LTR`, `ENCODER_LTR_*`,
`ENCODER_OPTION_PROFILE`, `ENCODER_OPTION_LEVEL`, `ENCODER_OPTION_NUMBER_REF`
and `ENCODER_OPTION_PADDING`. That is a silent-success stub on a public API and
it is the largest single item this phase leaves open; it was not touched here
because it is a behaviour change on the public surface, well outside rate
control, and worth doing with its own test pass.

Note in particular that C++'s `ENCODER_OPTION_RC_MODE` sets `iRCMode` **and**
calls the real `WelsRcInitFuncPointers` to re-point the dispatch table; setting
the field alone leaves the encoder running the previous mode's callbacks.

#### `compare.sh` grew two things

`compare.sh <yuv> <w> <h> <frames> <qp> <cabac> <gop> [rcmode] [baseinit]`.

- **`baseinit`** — 1 selects `Initialize(SEncParamBase)`, the path upstream's
  `BaseEncoderTest::InitWithParam` takes and the one the SHA-1 parity test
  exercises. It leaves every `FillDefault` value in place, so scene-change
  detection, background detection, adaptive quantisation and frame skip are all
  on, `eSpsPpsIdStrategy` is `INCREASING_ID` and `iTargetBitrate` is 5 Mbit/s.
  This is a materially wider configuration than the `InitializeExt` gate and it
  is now byte-exact too.
- A **non-zero driver exit is reported**. A debug-build Rust panic (the
  `iSumSad` overflow above) previously read as an ordinary byte difference,
  because the aborted process leaves a short file. Both driver exit codes are
  now printed with the log path.

> **Trap worth recording: the drivers take `out.264` *before* `rcmode`.** Half an
> hour of this session went into comparing a C++ run that was silently on
> `RC_QUALITY_MODE` — `atoi` of a file path is 0 — against a Rust run on the mode
> actually under test. If a dump looks like the C++ is ignoring a mode, check the
> argument order first.

#### The dumps

`OH264_RCDUMP` (per-frame `SWelsSvcRc`) and `OH264_RCMBDUMP` (per-macroblock
`SRCSlicing`) are new and permanent on the Rust side, alongside the four from
Phase 4.6. `OH264_MBDUMP` moved: it now prints **after**
`pfMdBackgroundInfoUpdate` rather than before, so `uiMbType` is the value the
syntax writer will actually see — which is what exposed the shadowed
`WelsMdUpdateBGDInfo`. Only the C++ half has to be re-patched next time.

> Two fields in `OH264_MBDUMP` are **not comparable**: `cc=` (`iCostChroma`) and
> `skip=` (`iCostSkipMb`). `SWelsMD` is an uninitialised stack local in C++
> (`svc_encode_slice.cpp`, `SWelsMD sMd;`) and a zeroed `Default` in Rust, so on
> any macroblock whose mode decision returns before setting them — every
> background-skip macroblock, for one — the C++ prints stale bytes and the port
> prints 0. Neither is read afterwards. Ignore both when diffing.

### Phase 5.2 — SetOption, CABAC, qp=0, and the duplicate audits — **DONE**

**CABAC is byte-identical, `SetOption` handles all 32 of upstream's options, the
qp=0 difference is closed, and `todo!()`/`unimplemented!()` are both zero in
`src/`.** Measured, not asserted:

| axis | range | configurations | result |
|---|---|---|---|
| `iRCMode` x input x GOP x init path, **CAVLC** | 5 modes x 4 inputs x GOP −1/2/8 x baseinit 0/1 | 120 | identical |
| the same, **CABAC** | as above | 120 | identical |
| QP x `iRCMode` x cabac x size | 52 QPs x 5 modes x cabac 0/1 x 3 inputs | 1560 | identical |
| **slice mode** x count x `iRCMode` x GOP x cabac x input | `SM_FIXEDSLCNUM_SLICE`/`SM_RASTER_SLICE` x 2/3/4/6 slices x 4 modes x GOP −1/2 x cabac 0/1 x 2 inputs | 256 | identical |

| check | result |
|---|---|
| `cargo build` | clean, 17 warnings, all `dead_code`/`unused` |
| `cargo test --no-fail-fast` | **292 passed, 0 failed, 20 ignored** (was 290/0/20) |
| decoder conformance | 53/53 |
| `codec_unittest` | 533/534, only `DecoderDeblocking.DeblockingInit` |
| `todo!()` / `unimplemented!()` in `src/` | **0 / 0** |

#### The harness was measuring nothing for CABAC — read this before trusting a sweep

Both drivers pinned `sSpatialLayers[0].uiProfileIdc = PRO_BASELINE`, and
`ParamValidationExt` (`encoder_ext.cpp:655`) resets `iEntropyCodingModeFlag` to 0
for a baseline layer: *"layerId(%d) Profile is baseline, Change CABAC to CAVLC"*.
So `cabac 1` produced a **CAVLC** stream in both encoders, byte for byte, and a
120-configuration "CABAC sweep" passed with 8034 == 8034 without a single CABAC
symbol being written.

Both drivers now select `PRO_HIGH` when the flag is set. The same configuration
immediately read 7833 vs 7850 and four defects fell out. **The lesson generalises:
a sweep that passes on the first run over a newly-enabled feature is evidence the
feature is not enabled.** Check that turning the knob changes the C++ output before
believing the port matches it.

> `baseinit=1` still exercises CAVLC whatever `cabac` says: `SEncParamBase` has no
> entropy field and `FillDefault` leaves `uiProfileIdc` at `PRO_UNKNOWN`, which
> `ParamValidationExt` resolves to `PRO_BASELINE` because the flag is 0. That is
> upstream's own path, so it is worth keeping — just do not count those 60
> configurations as CABAC coverage.

#### CABAC — three functions were missing, four more were wrong

The Phase-5.2 plan named three unported functions and it was right about all three:
`StashMBStatusCabac`, `StashPopMBStatusCabac` and `GetBsPosCabac`, all in
`set_mb_syn_cavlc.cpp` (*cavlc*, next to their CAVLC twins). It was also right that
`WelsCabacEncodeFlush`/`WelsCabacEncodeGetPtr` were already ported: the CABAC branch
of `WelsWriteSliceEndSyn` is a two-line call into them, so the tree's last
`unimplemented!()` retired for free and its stale message with it.

What the plan could not see, because nothing in the tree was exercising CABAC:

| defect | detail |
|---|---|
| **`WelsCabacInit` was a no-op.** | Its body was `if pCtx.is_null() { return; }` plus a comment pointing at `WelsCabacInitContexts` — which had **no call site**. `sWelsCabacContexts[4][52][460]` was never filled, so every context model started at `SStateCtx::default()`. `WelsInitEncoderExt` calls this on every CABAC configuration (`encoder_ext.cpp:2358`). |
| **`WelsCabacContextInit` was a no-op**, with the comment *"High-level slice loop supplies initialized contexts"*. Nothing did. |
| **`WelsInitSliceCabac` omitted the `WelsCabacContextInit` call** entirely (`svc_set_mb_syn_cabac.cpp:632`). |
| **`BsAlign` was declared a second time in `svc_set_mb_syn_cabac.rs` without the trailing `BsFlush`** (`svc_enc_golomb.h:120`). A local item beats an import, so `WelsInitSliceCabac` got the flushless one; `pBs->pCurBuf` still pointed before the pending accumulator word, and `WelsCabacEncodeInit` handed the arithmetic coder a buffer overlapping slice-header bytes that had already been written. **That is why the stream diverged at the first byte after the NAL header rather than in the residual** — a symptom that reads like a slice-header bug and is not one. |

Three of the four are shapes this port keeps producing: a stub under the correct
name, a faithful function with no call site, and a shadowing duplicate.

#### `SetOption` — 12 of 32, and six of the twelve were lying anyway

`CWelsH264SVCEncoder::SetOption` (`welsEncoderExt.cpp:692`) handles **32**
`ENCODER_OPTION_*` values and ends `default: return cmInitParaError`. The port
handled 12 and ended `_ => {}` followed by `return 0`.

The switch arms were the smaller half of the problem. Six helpers that the
**already-ported** options call were stub bodies under the correct name:

| helper | what it did | what the C++ does |
|---|---|---|
| `WelsEncoderApplyFrameRate` | clipped `fMaxFrameRate`, which the caller already does | pushes it into every dependency layer keeping each layer's output/input ratio (`encoder_ext.cpp:672`) |
| `WelsEncoderApplyBitRate` | `return 0` | re-splits `iTargetBitrate` across the layers in their existing ratio and verifies each (`:699`) |
| `WelsEncoderParamAdjust` | `*pSvcParam = *pCfg` | 296 lines deciding between folding the change in and a full uninit/init cycle (`:4182`) |
| `WelsEncoderApplyLTR` | set two fields | derives the reference count the LTR setting needs and re-adjusts (`:4479`) |
| `CheckReferenceNumSetting` | stored the value unchecked | out-of-range falls back to `AUTO_REF_PIC_COUNT`, it does not clamp (`:163`) |
| `WelsEncoderApplyBitVaryRang` | wrote the caller's own field | lowers each layer's `iMaxSpatialBitrate` and verifies (`:726`) |

So `ENCODER_OPTION_FRAME_RATE`, `ENCODER_OPTION_BITRATE` and
`ENCODER_OPTION_MAX_BITRATE` were *listed as handled* and were wrong. **A switch-arm
count is not a coverage measure.**

`GetOption` was closer but not exact: it answered `ENCODER_OPTION_TRACE_LEVEL`,
which C++ has no case for and rejects; it was missing `INTER_SPATIAL_PRED` and
`STATISTICS_LOG_INTERVAL`; and `GET_STATISTICS` dropped `fAverageFrameSpeedInMs`,
`uiIDRReqNum` and `uiLTRSentNum`.

`SetOption`'s match is now **exhaustive with no wildcard arm**, so an option that
goes unhandled is a compile error rather than a silent success. C++'s
`default: return cmInitParaError` has no testable counterpart — the Rust signature
takes a typed `ENCODER_OPTION`, so an out-of-range id is not constructible.

> **One deliberate deviation, in `rc_mode_from_raw`.** C++ casts the caller's
> `int32_t` straight into `RC_MODES`, so an out-of-range value is stored verbatim
> and `WelsRcInitFuncPointers`' switch (no `default`) leaves the dispatch table on
> the previous mode. A Rust enum cannot hold a value outside its variants; an
> unrecognised mode becomes `RC_QUALITY_MODE` (C++'s 0). Every value the reference
> accepts round-trips exactly.

`test_set_get_option_matches_cxx_for_every_option` asserts a **measured** table: a
probe linked against `libopenh264.a` drove the same sequence on the same 160x96
configuration and printed every return code and every field written. It covers
`GetOption` for all 32 ids and `SetOption` for all 20 that were missing, including
the full uninit/init reset `ENCODER_OPTION_LTR` triggers.

#### qp=0 was one wrong constant

`svc_encode_slice.rs:73` declared `DELTA_QP = 1`; `rc.h:77` says **2**.
`UpdateQpForOverflow` is its only reader, so a macroblock that overflowed the CAVLC
level suffix was re-encoded one QP step below the reference's choice. That path is
CAVLC-only (`svc_encode_slice.cpp:571` guards it on `!iEntropyCodingModeFlag`) and
only fires at very low QP on pictures whose macroblocks actually overflow — which is
exactly the observed signature: 160x96 and 1280x720 identical at qp=0, 320x192 and
152x100 not, and CABAC identical everywhere.

The diagnosis came from the size sweep, not from a dump: *CAVLC-only + qp-only +
size-dependent* names one code path, and there is only one CAVLC-only branch in the
I-slice loop.

#### Both audits now compare contents, not names

Seven phases running, a duplicated constant has held a wrong value. Names are free;
values are the evidence.

`find_dup_types.sh` was encoder-only and matched types. It now scans
`src/{encoder,common,decoder,processing}` and reports **types** (exact and
case-insensitive), **`pub type` aliases**, **tables**, and **scalar constants
compared by value** — its four documented blind spots, each of which had already
hidden a real bug. `pub use` re-exports are not counted, because re-exporting one
definition is the fix it asks for.

> A table comparison must **strip comments before scraping elements**. Without
> that it reports ten false positives, because `/* 0 */` row markers scrape as
> data — `g_kiQuantInterFF` looked like 531 elements against 523 and is identical
> row for row.

What the value comparison found, beyond `DELTA_QP`:

| constant | wrong copy | header | live? |
|---|---|---|---|
| `MAX_FRAME_RATE` | `param_svc.rs` 30.0 | `wels_const.h:60` = **60** | **yes** — that copy is the one `FillDefault` and both transcoders use, so `GetDefaultParams` reported 30 fps and `ParamTranscode`'s `WELS_CLIP3` capped any caller above 30. Measured: the reference returns 60.0 and keeps 50.0 through `InitializeExt`. |
| `MAX_SLICES_NUM_TMP` | `param_svc.rs` 32 | `codec_app_def.h:56` = **35** | **yes** — both transcoders take `min (MAX_SLICES_NUM, MAX_SLICES_NUM_TMP)` (`param_svc.h:203`), so `uiSliceNum` was capped three slices low. |
| `MAX_PPS_COUNT` | `param_svc.rs` 256 (the decoder's) | `wels_const.h:51` = **57** | no — only `SExistingParasetList` used it, whose `sPps` was oversized by 199. |
| `WELS_CPU_*` | eight modules with disagreeing subsets: `WELS_CPU_NEON` seven distinct values across eight copies, `WELS_CPU_LSX` five across six | `cpu_core.h:46-98` | dead while `WelsCPUFeatureDetect` returns 0; live the moment any SIMD dispatch is. One definition now in `common/cpu_core.rs`, with a test against the header. |
| `MB_TYPE_*` | `decoder_core.rs`, whole set shifted one bit down (`16x16 = 0x2`, `SKIP = 0x40`) | `wels_common_defs.h:278-283` (`0x8` … `0x100`) | the match in `CheckRefPicturesComplete` reads them; error-free conformance streams never take that path. |
| `MB_TYPE_DIRECT` | `svc_set_mb_syn_cavlc.rs` 0x200 (that is `MB_TYPE_INTRA_PCM`) | `:286` = **0x800** | no, unread there. |
| `RECIEVE_FAILED` | `wels_preprocess.rs` 0 | `wels_const.h:153` = **2** | no, unread there. |
| `GOM_H_SCC` | `wels_preprocess.rs` 2 | `rc.h:57` = **8** | reachable only through the unported screen-content analyser. |

Two truncated **tables**, both in `decoder/decode_slice.rs`: `g_kuiMbCountScan4Idx`
and `g_kuiCache48CountScan4Idx` are `[24]` in `common_tables.cpp` and were `[16]`
here — the chroma half missing. Nothing in that module indexes past 15, so both were
latent; `g_kuiGolombUELength` in Phase 4.6 was the same shape and *did* index out of
bounds.

Three more shadowing duplicates, all the `WelsMdUpdateBGDInfo` class, all invisible
at one slice per frame — which is every configuration the harness can currently
drive:

| name | the shadow | the real one |
|---|---|---|
| `WelsInterMbEncode` | 557 chars in `svc_encode_slice.rs`: did the DCT, dropped quantisation and reconstruction. Dead — all three call sites resolved to the real one — but a landmine in the file it sat in. | `svc_mode_decision.rs` |
| `WelsGetNextMbOfSlice` | `deblocking.rs`: returned `kiMbXY + 1` bounded only by the frame, ignoring `sSliceEncCtx` and `pOverallMbMap`. Agrees for `SM_SINGLE_SLICE`, walks across slice boundaries otherwise. `deblocking.cpp:733` calls the real one. | `svc_encode_slice.rs` |
| `GetCurrentSliceNum` | `deblocking.rs`: returned a hardcoded `1`, and `WelsDeblockingFilterMbAvcbase`'s slice loop reads it (`deblocking.cpp:754`), so deblocking only ever filtered slice 0. | `svc_encode_slice.rs` |

**`find_stub_bodies.py --dups` reports 87 duplicated function names.** The ones
inspected and deleted are above; the ones inspected and kept are methods on distinct
types (`Process`, `Init`, `Set`, `Get`, `Execute`, `QueueTask`, and the thread-pool
container helpers `begin`/`erase`/`push_back`/`pop_front`), the `Bs*` writers
(compared line by line in Phase 3), and the `WelsI16x16LumaPred*_sse2`/`_neon` pairs
(all unassigned on this target). **The rest have not been read.** Work down the list
worst-disparity first; three of the four inspected this phase were defects.

`find_dup_types.sh` is **not silent** and cannot be made silent without violating the
"do not unify encoder and decoder" rule: it reports 23 types, 40 aliases, 29 tables
and 38 constants, and the overwhelming majority are one encoder declaration beside
one decoder declaration of a name the two codecs genuinely keep separate
(`SDqLayer`, `SMbCache`, `SLogContext`, `PGetIntraPredFunc`, the deblocking tables,
`MAX_PPS_COUNT`). Of the encoder-vs-encoder pairs, every constant now agrees by
value and every table now agrees element for element except the three deblocking
tables, which are `static const` file-local in C++ and deliberately `[52 + 12]` in
the encoder against `[52 + 24]` in the decoder — annotated in both sources.

#### The multi-slice modes work, and `compare.sh` can now drive them

`compare.sh` gained a tenth and eleventh argument:

```bash
compare.sh <yuv> <w> <h> <frames> <qp> <cabac> <gop> [rcmode] [baseinit] [slicemode] [slicenum]
```

`slicemode` is 0 `SM_SINGLE_SLICE` (default), 1 `SM_FIXEDSLCNUM_SLICE`,
2 `SM_RASTER_SLICE`, 3 `SM_SIZELIMITED_SLICE`; `slicenum` is the slice count for 1
and 2, the rows-per-slice for 2, and the byte constraint for 3.

`SM_FIXEDSLCNUM_SLICE` and `SM_RASTER_SLICE` were **byte-identical on the first
run**, 256/256. That is the interesting part: the two shadowed helpers this phase
deleted — `WelsGetNextMbOfSlice` returning `kiMbXY + 1` and `GetCurrentSliceNum`
returning a hardcoded `1` — were both wrong for exactly these modes and both
invisible at one slice. Had the drivers grown this argument before the `--dups`
sweep, the sweep would have been a debugging session instead of a clean pass.

`SM_SIZELIMITED_SLICE` is still refused: `WelsEncoderEncodeExt` returns
`ENC_RETURN_UNSUPPORTED_PARA` (it needs `WelsCodeOnePicPartition` and
`WelsInitCurrentDlayerMltslc`).

> **The refusal is invisible to the caller, and that is upstream's doing, not the
> port's.** `EncodeFrameInternal` (`welsEncoderExt.cpp:404`) maps only
> `ENC_RETURN_MEMALLOCERR`/`MEMOVERFLOWFOUND`/`VLCOVERFLOWFOUND` and
> `ENC_RETURN_INVALIDINPUT` to failures; its third arm is
> `(kiEncoderReturn != ENC_RETURN_SUCCESS) && (kiEncoderReturn == ENC_RETURN_CORRECTED)`,
> which is a typo for `!=` upstream and makes every other non-zero code fall through
> to `cmResultSuccess`. The port copies that statement for statement, so
> `SM_SIZELIMITED_SLICE` reports success and emits nothing. If you want the harness
> to distinguish "refused" from "produced zero bytes", check the driver's frame
> count, not its return code.

### Phase 5.3 — the slice modes, PSNR, and the MT ground truth — **DONE**

**All four slice modes are byte-identical, CAVLC and CABAC.** `SM_SIZELIMITED_SLICE`
was the last configuration the encoder refused outright.

| axis | configurations | result |
|---|---|---|
| `iRCMode` x input x GOP x init path, CAVLC | 120 | identical |
| the same, CABAC | 120 | identical |
| QP x `iRCMode` x cabac x size | 1560 | identical |
| `SM_FIXEDSLCNUM_SLICE`/`SM_RASTER_SLICE` x 2/3/4/6 slices x `iRCMode` x GOP x cabac x input | 256 | identical |
| `SM_SIZELIMITED_SLICE` x 5 byte constraints x 2 QPs x `iRCMode` x GOP x cabac x input | 320 | identical |

| check | result |
|---|---|
| `cargo test --no-fail-fast` | **293 passed, 0 failed, 20 ignored** |
| decoder conformance | 53/53 |
| `codec_unittest` | 533/534, only `DecoderDeblocking.DeblockingInit` |
| `todo!()` / `unimplemented!()` in `src/` | 0 / 0 |

#### The machinery was the easy half

Six functions were genuinely missing and are straightforward transcriptions:
`UpdateSlicepEncCtxWithPartition`, `WelsInitCurrentQBLayerMltslc`,
`DynslcUpdateMbNeighbourInfoListForAllSlices`, `WelsInitCurrentDlayerMltslc`,
`DynSliceRealloc`, `WelsCodeOnePicPartition`. `RequestMemorySvc` also refused
`bDynamicSlice && iEntropyCodingModeFlag`; `pDynamicBsBuffer` is now allocated,
which is what `sDss.pRestoreBuffer` needs — CABAC renormalisation rewrites bytes
already emitted, so stepping back over a slice boundary has to restore the *bytes*
as well as the coder state.

**The expensive half was the two dynamic-slicing macroblock loops, which had been
ported long ago, were never reachable, and were wrong.** Neither was a stub in the
"returns early" sense: both were long, plausible and structurally right.

| function | what was missing |
|---|---|
| `WelsISliceMdEncDynamic` | `WelsMdIntraInit` **and `WelsMdIntraMb`** — no mode decision at all. The first frame came out 42 bytes against the reference's 3525. |
| `WelsMdInterMbLoopOverDynamicSlice` | `WelsMdIntraInit`, `WelsMdInterInit`, `WelsMdInterSaveSadAndRefMbType`. `WelsMdInterInit` installs the reference-block pointers in `pMbCache`, so `pfInterMd` read a null `pSample2` and the encoder aborted on the first P frame. |
| `WelsPSliceMdEncDynamic` | `sMd.bMdUsingSad = (iComplexityMode == LOW_COMPLEXITY)`. **The identical defect was found and fixed in its non-dynamic twin `WelsPSliceMdEnc` in an earlier phase.** The twin kept it because nothing ran it. |
| `UpdateMbNeighbourInfoForNextSlice` | C++ is a `do`-`while`; the port a `while`, so the first macroblock went unupdated whenever a boundary landed on the last macroblock of a partition. |
| `AddSliceBoundary` | open-coded `for i in 0..count as usize` where C++ calls `WelsSetMemMultiplebytes_c` with a **signed** count; a negative count became ~2^64 iterations. |

> **The lesson, and it is the same one Phase 5.2 learned about CABAC.** A fix
> applied to one of a pair of near-identical functions does not reach the other,
> and nothing in the tree notices while the other is unreachable. When you fix a
> defect in `Foo`, grep for `FooDynamic`, `FooExt`, `Foo_c` and the enhancement-layer
> variant **before** moving on. Three of the five defects above are that shape.

#### `WelsCalcPsnr`

Ported into `common/wels_common_defs.rs` with `CONST_FACTOR_PSNR` and `CALC_PSNR`,
and wired into `WelsEncoderEncodeExt`. Two of its return values are sentinels rather
than PSNRs: `99.99` for an exact match and `-1.0` for a null plane.

> The reference's PSNR block is asymmetric and it is not a transcription slip: each
> plane is **computed** when either `pSvcParam->bPsnrX` or `pSrcPic->bPsnrX` is set,
> but only **reported** when `pSrcPic->bPsnrX` is set. Asking through `SEncParamExt`
> alone pays for a full-frame scan and reports nothing.

`test_wels_calc_psnr_matches_cxx` asserts six measured values from a probe linked
against `libopenh264.a`, including both sentinels and a mismatched-stride case.

#### `ParamBaseTranscode`

Carried since Phase 4 and now fixed. Five fields (`uiProfileIdc`, `uiLevelIdc`,
`iSpatialBitrate`, `iMaxSpatialBitrate`, `iDLayerQp`) are written through
`sSpatialLayers->`, which is `sSpatialLayers[0]`, on **every** iteration; the port
wrote `[iIdxSpatial]` as well. Writing both is not a superset: beyond one spatial
layer the reference leaves `[1..]`'s profile, level, max bitrate and QP at whatever
`FillDefault` left, and `[0]`'s profile ends at `PRO_SCALABLE_BASELINE` because the
last iteration rewrites it.

#### Multi-threading: the ground truth, measured

`compare.sh` gained a thirteenth argument, `iMultipleThreadIdc`. The status doc used
to list "is the C++ even deterministic here?" as the question blocking this work.
It is answered:

- **The C++ multi-threaded encoder is deterministic.** Three runs each at
  `iMultipleThreadIdc` 2 and 4 produced identical bytes, for both
  `SM_FIXEDSLCNUM_SLICE` and `SM_SIZELIMITED_SLICE`. `compare.sh` is a valid oracle.
- **Multi-threaded output differs from single-threaded** for the same nominal
  configuration — 8324 bytes against 8322 at `SM_FIXEDSLCNUM_SLICE`/4 slices.
  Threading is not a scheduling detail here: it changes the slice partitioning and
  therefore the bitstream. A port must reproduce that difference, not merely add
  threads.

### Phase 5.4 — what is still not exact — **NOT STARTED**

1. **`iMultipleThreadIdc > 1`.** The last configuration the encoder refuses. More of
   it exists than the older notes suggested — `slice_multi_threading.rs` (903 lines),
   `wels_task_management.rs` (865) and `common/wels_thread_pool.rs` (905) are all
   present, against 948 lines of C++ in the two source files, and
   `InitAllSlicesInThread`, `SliceLayerInfoUpdate`, `AdjustBaseLayer`,
   `AdjustEnhanceLayer`, `CreateTaskManage` and `AppendSliceToFrameBs` all have
   bodies. What is missing:
   - `RequestMtResource` (`slice_multi_threading.rs:550`) allocates `SSliceThreading`,
     `pThreadPEncCtx` and `pThreadBsBuffer` and stops. The C++ (111 lines against the
     port's 55) then opens four event sets per thread — `pUpdateMbListEvent`,
     `pFinUpdateMbListEvent`, `pSliceCodedEvent`, `pReadySliceCodingEvent` — and
     creates the threads. **Treat every one of those existing bodies as unverified**:
     Phase 5.3 found that both dynamic MB loops, ported and plausible, were wrong
     because nothing had ever run them. The MT bodies are in exactly that state.
   - `pTaskManage` is never created in `WelsInitEncoderExt`, and the MT branch of
     `WelsEncoderEncodeExt` (`encoder_ext.cpp:3714`, `pCtx->pTaskManage->ExecuteTasks()`
     then `AppendSliceToFrameBs`) returns `ENC_RETURN_UNSUPPORTED_PARA` at
     `encoder_ext.rs`'s two MT sites, plus the `AdjustEnhanceLayer`/`AdjustBaseLayer`
     load-balancing arm under `SM_FIXEDSLCNUM_SLICE`.

   Drive it with `compare.sh … <slicemode> <slicenum> <threads>`, and compare against
   the **multi-threaded** C++, not the single-threaded one.
2. **`METHOD_DOWNSAMPLE`**, which is what blocks more than one spatial layer — the
   largest untested area of the encoder. `ParamBaseTranscode` is now correct for
   multi-layer, so that pre-requisite is out of the way.
3. **`SCREEN_CONTENT_REAL_TIME`**: `METHOD_SCENE_CHANGE_DETECTION_SCREEN`,
   `METHOD_COMPLEXITY_ANALYSIS_SCREEN` and `METHOD_SCROLL_DETECTION` interlock (the
   screen scene-change detector reads the scroll detector's result);
   `PreprocessSliceCoding` does not translate `encoder_ext.cpp:2708-2771`;
   `AllocPicture` refuses `iNeedFeatureStorage != 0` rather than calling the unported
   `RequestScreenBlockFeatureStorage`; `RequestMemorySvc` refuses the
   `SVAAFrameInfoExt`/`RequestMemoryVaaScreen` allocation. `GOM_H_SCC` is corrected
   and waiting.
4. **`METHOD_DENOISE`.**
5. **The three `SPS_LISTING` parameter-set strategies.** `CreateParametersetStrategy`
   returns null; `paraset_strategy.rs`'s C-style vtable is the established pattern,
   and `WelsEncoderParamAdjust`'s `OutputCurrentStructure`/`LoadPreviousStructure`
   calls are live for the non-`CONSTANT_ID` strategies, so the `SPS_LISTING`
   overrides of those two are what the new strategies need first.
6. **`find_stub_bodies.py --dups`: 87 names, most unread.** Four of the seven
   inspected across Phases 5.2 and 5.3 were defects. Still the highest-yield
   unfinished audit in the tree.
7. The `Combined3` SIMD branches of `WelsMdI16x16` / `WelsMdI4x4` /
   `WelsMdIntraChroma` are not translated; each site calls `assert_no_combined3`,
   which panics if the slot is ever non-null. Measured NULL on this target. Adding
   SIMD dispatch makes these three live, and `common/cpu_core.rs` load-bearing, the
   same moment.

### Phase 6 — cleanup — **about a third done**

Done in Phase 5.2:

- **The encoder's duplicated constants agree by value.** Seven held wrong values and
  are fixed; see *Both audits now compare contents, not names*.
- **Duplicated tables agree element for element**, except the three deblocking
  tables that are deliberately different sizes in the C++ and are annotated in both
  sources. Two truncated tables were fixed.
- **`find_dup_types.sh` compares values, not names**, and covers types, aliases,
  tables and scalar constants across all four source directories.

> **A correction to what this section used to claim.** It said "every duplicated
> `pub const` in `src/` now holds the same value in every module". That is true for
> `src/encoder/`; it is **not** true for `src/`. Measured now: **38 duplicated
> constants still differ by value text.** Roughly half are notation only (`1` vs
> `0x01`, `1 << 1` vs `2`, `MAX_REF_PIC_COUNT + 1` vs `17`, `0x08: u8` vs
> `0x00000008: u32`); several are legitimately different between the codecs
> (`CHROMA_AC`/`CHROMA_DC`, `MAX_PPS_COUNT`, `MAX_SHORT_REF_COUNT`); **19 are
> decoder `ERR_INFO_*`/`ERR_LEVEL_*` codes that genuinely disagree** across decoder
> modules, and **two are genuinely wrong and live**:
>
> | constant | wrong copy | header | effect |
> |---|---|---|---|
> | `MAX_MB_SIZE` | `decoder/parameter_sets.rs` = 1024 | `dec_golomb.h:338` = **36864** | `nalu.rs:1481` rejects an SPS with `iMbWidth > 1024`; the reference allows 36864. Stricter than the reference, so it rejects nothing real — but it is a divergence, not a hardening. |
> | `MAX_NAL_UNIT_NUM_IN_AU` | `decoder/decoder_core.rs` = 1024 | `wels_const.h:59` = **32** | sizes the access-unit NAL list (`MemInitNalList`) and seeds `uiCountUnitsNum`, so the port allocates 32x the reference and grows on a different schedule. |
>
> Both are decoder-side and both survive conformance, which is exactly why they are
> still here.

What is left:

- **Collapse the remaining duplicated *names*.** Agreeing values is not one
  definition. `find_dup_types.sh` reports **130**: 23 types, 40 aliases, 29 tables,
  38 constants. Most are one encoder declaration beside one decoder declaration of a
  name the two codecs genuinely keep separate; those need a one-line comment, not a
  merge. The encoder-vs-encoder pairs can be re-exported the way `MAX_PPS_COUNT`,
  `MAX_FRAME_RATE`, `DELTA_QP`, `MAX_SLICES_NUM`, `GOM_H_SCC`, `RECIEVE_FAILED` and
  the whole `WELS_CPU_*` set now are.
- **`find_stub_bodies.py --dups`: 90 names, most unread.** Four of the seven
  inspected across Phases 5.2 and 5.3 were defects. This is the highest-yield
  unfinished audit in the tree.
- **Fold the module-level `#![allow(dead_code, unused_variables, …)]` blankets back
  to the narrowest scope that still compiles. 67 modules carry one; 63 of those
  silence `dead_code`.** These hide exactly the warning — `dead_code` on a function
  that should have a caller — that would have flagged `GomRCInitForOneSlice`'s
  missing call site, `WelsCabacInitContexts` and `WelsCabacContextInitFromContexts`,
  all of which were faithful bodies nothing called.

#### The dominant defect class, restated after seven phases

**A function that exists is not a function that works, and a function that works is
not a function that runs.** Every phase since 4.5 has found more instances, and by
now the shapes are enumerable:

| shape | example | what finds it |
|---|---|---|
| a body that does a fraction of the work under the correct name | `WelsEncoderApplyFrameRate`, `WelsEncoderParamAdjust`, `WelsCabacInit` | `find_stub_bodies.py`'s call-set audit; reading the C++ beside it |
| a faithful body with **no call site** | `GomRCInitForOneSlice`, `WelsCabacInitContexts`, `WelsCabacContextInitFromContexts` | nothing automated — only diffing the *caller* against the C++. Narrowing the `dead_code` allows would surface these. |
| a faithful body **shadowed** by a same-named one in the module that uses it | `WelsMdUpdateBGDInfo`, `BsAlign`, `WelsGetNextMbOfSlice`, `GetCurrentSliceNum`, `WelsInterMbEncode` | `find_stub_bodies.py --dups` |
| a correct function reached through a **wrong constant** | `GOM_SAD`/`GOM_VAR`, `DELTA_QP`, `MAX_FRAME_RATE`, `g_kuiGolombUELength` | `find_dup_types.sh`'s value comparison |
| a correct function under a **configuration that never runs** | the whole CABAC path, silently downgraded to CAVLC by `PRO_BASELINE` | checking that the knob changes the *C++* output |

The last row is new in Phase 5.2 and is the one no audit of the Rust can catch.


---

### Phase 5.4 — `iMultipleThreadIdc > 1`, the last refused axis

**Result: byte-identical to the multi-threaded C++ across 120 configurations** —
3 inputs x `iMultipleThreadIdc` {2,4} x 5 slice specs (`SM_FIXEDSLCNUM_SLICE` 2 and
4 slices, `SM_RASTER_SLICE` 3, `SM_SIZELIMITED_SLICE` 1500 and 600 bytes) x CAVLC
and CABAC x `RC_OFF_MODE` and `RC_QUALITY_MODE`. The single-threaded sweep was
re-run afterwards and is unchanged.

**No configuration axis the harness can drive is refused any more.**

#### The oracle, re-measured

Three runs each of nine configurations, `cxx_enc` only:

| | threads=1 | threads=2 | threads=4 |
|---|---|---|---|
| `SM_FIXEDSLCNUM_SLICE`, 2 slices | 13145 | 13165 | 13165 |
| `SM_FIXEDSLCNUM_SLICE`, 4 slices | 13163 | 13319 | 13319 |
| `SM_SIZELIMITED_SLICE`, 1500 B | 13305 | 13372 | 13290 |

Every cell was identical across its three runs, so `compare.sh` is a valid oracle.
Two things the previous handoff did not record:

- **For `SM_FIXEDSLCNUM_SLICE`, threads=2 and threads=4 produce the same bytes.**
  Turning MT *on* changes the bitstream; the thread *count* does not. Only
  `SM_SIZELIMITED_SLICE` varies with it, because `iActiveThreadsNum` sets the
  partition count.
- **The C++ is deterministic here only because `cxx_enc.cpp:82` pins
  `bUseLoadBalancing = false`.** With it on, `DynamicAdjustSlicing` re-slices from
  `uiSliceConsumeTime` — wall-clock microseconds — and upstream's own API doc says
  so: "will change slicing of a picture during the run-time of multi-thread
  encoding, so the result of each run may be different" (`codec_app_def.h:579`).
  The load-balancing arm is therefore **not byte-verifiable by construction**. It is
  ported (`AdjustBaseLayer`/`AdjustEnhanceLayer` at `encoder_ext.cpp:3573`) but only
  reachable by a caller that sets the flag; both C++ call sites gate on it.

#### Why the port refused, and what was actually wrong

`RequestMtResource` is **smaller** than the handoff claimed, not larger. It creates
no threads — `slice_multi_threading.cpp:327` zeroes `pThreadHandles[iIdx]`, and every
worker comes from the shared `CWelsThreadPool` that `CreateTaskManage` acquires. And
the four per-thread event sets (`pUpdateMbListEvent`, `pFinUpdateMbListEvent`,
`pSliceCodedEvent`, `pReadySliceCodingEvent`), `pSliceCodedMasterEvent` and
`mutexEvent` are opened and closed but **never signalled or waited on anywhere in
`codec/`** — vestiges of the pre-thread-pool design. Only `mutexSliceNumUpdate`,
`mutexThreadBsBufferUsage`, `mutexThreadSlcBuffReallocate` and `mutexEncoderError`
are live. The port reproduces those four and skips the dead ones.

Four defects, one per shape in the table above:

1. **The whole task hierarchy was hollow** (*a fraction of the work under the correct
   name*). `CWelsSliceEncodingTask::Execute()` called `self.base.Execute()`, which
   only signalled the sink. No `InitTask`, no `WelsCodeOneSlice`, no `WriteSliceBs`,
   no deblocking — not one slice was ever encoded by a task.
   The cause is that C++ inheritance had been modelled by casting `*mut Derived` to
   `*mut CWelsBaseTask`. Rust has no vtable there, so **every virtual call resolved
   to the base**. The same cast also made `DestroyTaskList`'s `Box::from_raw` free
   the wrong type. Replaced by one struct with an `ETaskKind` discriminant, which
   reproduces the vtable exactly.
2. **A second `CWelsThreadPool`** (*shadowed by a same-named one*).
   `wels_task_management.rs` declared its own, whose `QueueTask` ran the task inline
   on the calling thread; that is the one `CWelsTaskManageBase::Init` used. The real
   905-line pool in `common/wels_thread_pool.rs` had **no caller outside its own
   tests**. `find_stub_bodies.py --dups` lists this pair.
3. **The real pool deadlocked whenever tasks outnumbered threads** — so it would have
   failed the moment it was first used regardless. Both wait loops
   (`CWelsTaskThread::Start` and `CWelsThreadPool::Start`) test a predicate guarded
   by one lock (`m_pTask`, the waited-task list) while waiting on a condvar paired
   with a *different* mutex (`m_mutex`), and both `SetTask` and `SignalThread`
   notified without holding it. A task queued in that window is lost forever: the
   worker parks holding a task it never runs. Notifications now take `m_mutex`.
4. **`CWelsConstrainedSizeSlicingEncodingTask` derives from
   `CWelsLoadBalancingSlicingEncodingTask`**, not from `CWelsSliceEncodingTask`
   (`wels_task_encoder.h:110`), so it inherits the slice-timing `InitTask`/
   `FinishTask` overrides too. The first draft of the discriminant dispatch missed
   this.

Also live again: **`pTaskManage->InitFrame`** (`encoder_ext.cpp:2619`) had been
skipped in `WelsInitCurrentLayer` under a comment asserting `pTaskManage` is always
null. True while MT was refused; false now.

#### How defect 3 was found

Not by reading. A sweep run hung at `SM_FIXEDSLCNUM_SLICE`/4 slices/2 threads —
4 tasks, 2 workers — at 0% CPU. `sample <pid>` showed the shape immediately: main
thread in `WelsTaskBarrier::wait_for_completion`, **both workers parked idle**, the
dispatcher asleep, tasks stranded in the queue. Idle workers plus a non-empty queue
is a lost wakeup and nothing else. Worth remembering: for a hang, one stack sample
beats any amount of instrumentation.

#### Deviations

- `WelsMutexInit`/`WelsMutexDestroy` are new in `slice_multi_threading.rs`, and
  lock/unlock is expressed as one scoped `with_wels_mutex(handle, closure)` call
  rather than two. A `std::sync::Mutex` guard owns the lock, so a bare
  `lock()`/`unlock()` pair cannot be spelled directly. Every C++ lock/unlock pair in
  the encoder brackets one straight-line region, so the critical sections are
  identical.
- `UpdateMbListNeighborParallel` is a `do`-`while` in C++ and a `while` in the port
  (pre-existing, not introduced here). They differ only when a slice holds <= 0
  macroblocks, where the C++ reads one macroblock out of bounds. Left as a `while`
  rather than reproducing an out-of-bounds read; no valid configuration reaches it.

#### Two tests changed

`test_task_manage_one_sync` and `test_task_manage_base_lifecycle` called
`ExecuteTasks` on a `Default` `sWelsEncCtx`. That passed only because the tasks were
hollow; with real bodies it is a null dereference in a pool worker, which aborts the
process. Both now assert what `Init` is responsible for — that the task lists are
built — and leave execution to the differential harness, which covers it far better.
Neither is `#[ignore]`d. A third, `test_task_list_operations`, was **missing its
`#[test]` attribute** and had never run; it passes now, which is why the suite went
from 293 to 294.

#### Phase 6 progress: three modules de-blanketed

`dead_code` removed from the module-level `#![allow(...)]` in
`encoder/wels_task_management.rs`, `encoder/slice_multi_threading.rs` and
`common/wels_thread_pool.rs`. **Zero new warnings** — the blanket was hiding nothing
in these three, now that the pool has a caller and the tasks have bodies. 3 of 63
done; the warning total is unchanged at 15, all `unused_assignments`,
`unused_mut` and `unreachable_pattern` elsewhere.

Worth noting for whoever continues this: had `dead_code` been live on
`common/wels_thread_pool.rs` earlier, it would have reported the entire 905-line
pool as unused and the shadowing `CWelsThreadPool` would have been found by a
compiler warning rather than by a deadlock.

### Phase 10 — `SCREEN_CONTENT_REAL_TIME` — **P10.1 + P10.2 DONE (2026-09-02); P10.3-P10.4 outstanding**

The last public usage type the port refused. Session P10.1 did two things, in
order, at `6f955eff` .. the session's docs commit (branch `rust3`):

**The referee first.** Both diffharness drivers gained `usage` and `lossless`
arguments, `compare.sh` carries them (positional: `setopt` must be spelled `0`
whenever `usage` is), `gen_screen_clip.py` makes deterministic scrolling-text I420
clips, and `sweep.sh scc` runs 148 configurations over seven inputs (`SCC_TIER=min`
for the 28-row byte tier). Baseline before the fence came down: `PASS=0 FAIL=148`,
every row `!! rust_enc exited 101` (`InitializeExt returned 1`), in both profiles.
A camera row with the new arguments spelled out stayed byte-identical.

**Then the fence.** The three port-added refusals that stood where the C++
allocates — `RequestMemorySvc`'s VAA extension, `InitDqLayers`'s feature-search
preparation, `AllocPicture`'s feature storage — are ported: `sWelsEncCtx::pVaa` is
an `Option<Box<VaaBlock>>` with `Base`/`Screen` arms (D-scc-1),
`SComplexityAnalysisScreenParam` lost its raw `int*` so the extension is `Sync`
(D-scc-2, enforced by the fork's `thread::scope` — F315), `SFeatureSearchPreparation`
is back on the last DQ layer with its scratch as a `Vec<u16>` and the storage's alias
of it deleted (D-scc-3), and every screen `scc` row now encodes to completion on both
sides with every Rust stream decoding: `PASS=0 FAIL=148`, zero driver exits, every
row `RESULT: DIFFER` — the bytes differ on every P frame because the three screen
video-processing plugins and the dispatch block are still unported. The camera
sweeps did not move by a byte (583/583 in both profiles at every checkpoint).

**Not done, by the user's ruling:** the Miri probe of D-scc-5 ("don't run miri,
translate to safe Rust directly"); the claim rests on `vaa_block_is_sync`, a
compile-time assertion.

Findings F315-F320 in [`phase10_findings.md`](phase10_findings.md).

**Session P10.2 — the three plugins**, at `1af668a4` .. the session's docs commit.

*A referee that could be green before the bytes can.* Under `SCC_TIER=min` rate
control is off, so both sides code every macroblock at QP 26 and nothing the screen
preprocessor reads depends on the coded bytes: its inputs are identical frame by
frame while the bitstreams differ by design. So the *sequence of scene-change
verdicts* must match three checkpoints before P10.3's byte gate.
`rust/tools/diffharness/scc_verdicts.sh` diffs the C++'s own
`iVaaFrameSceneChangeIdc = %d,codingIdx = %d` DEBUG line between the two encoders
(both drivers took an `OH264_TRACE_LEVEL` knob for it), and on LTR rows the five
`WelsBuildRefListScreen()` lines with it. Calibrated at three points: **0/28** with
every Rust extract empty, **18/28** once the trace line was ported and before the
plugins were, **28/28** once they were wired — plus **20/20** on the wide tier's LTR
rows, so the reference *selection* matches and not only the verdict.

*The plugins.* `METHOD_SCROLL_DETECTION` (new
`processing/scroll_detection.rs`), `METHOD_SCENE_CHANGE_DETECTION_SCREEN` (beside the
video detector) and `METHOD_COMPLEXITY_ANALYSIS_SCREEN` (at the foot of
`processing/complexity_analysis.rs`), each under its C++ name with in-file unit
tests, and all three call sites wired: no `RET_NOTSUPPORTED` on a live path, and
`processing/mod.rs` lists no untranslated method the encoder calls. All safe — no
new `unsafe`, allow or raw pointer, and the two new files carry
`#![forbid(unsafe_code)]`. The complexity plugin has a referee of its own: the first
P frame's `iFrameComplexity` on the five `rc=1` wide-tier rows, equal to the
reference's on all five (140312 / 420586 / 71452 / 2704751 / 10217697).

Camera sweeps unmoved throughout: 583/583 in both profiles at every checkpoint. The
census's `missing` fell 14 -> **2** (both P10.3's) and the
`SCREEN_CONTENT(dormant: Phase 10)` tags 30 -> **20**, ten retired against a measured
entry count rather than an argument (F328: `SvcMdSCDMbEnc`, which read 0 in all 48
`bg` rows, encodes >= 11000 macroblocks now, and both P-skips return true).

**Not done, by the user's ruling:** the Miri probe of D-scc-5 ("don't run miri,
translate to safe Rust directly"); the claim rests on `vaa_block_is_sync`, a
compile-time assertion. No Miri was run in P10.2 either.

Findings F321-F328 in [`phase10_findings.md`](phase10_findings.md). Next: P10.3 (the
dispatch — `PreprocessSliceCoding`'s screen block, `SetMeMethod`, the FME switch
family, the real `SetScrollingMvToMd`; the first byte gate on `SCC_TIER=min`), P10.4
(widen and close; add `scc` to `gates.sh`'s family list when it passes).
