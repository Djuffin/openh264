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
