# OpenH264 Testing Infrastructure

This document provides a comprehensive survey, inventory, and classification of the testing infrastructure in OpenH264.

---

## 1. Test Infrastructure Overview

| Component | Technology / Location | Role |
| :--- | :--- | :--- |
| **Core Test Framework** | **Google Test (gtest)** | C++ assertion and test runner framework driving all unit and integration tests |
| **Build Systems** | `Makefile` (`build/gtest-targets.mk`) & Meson (`test/meson.build`) | Targets for building test executables (`codec_unittest`) and libraries |
| **Test Assets** | `res/` | 50+ H.264 bitstreams (`.264`, `.jsv`) and raw YUV sequences (`.yuv`) |
| **Automation Runners** | `autotest/` | Shell and JS scripts for bit-exact comparison, performance profiling, and XML test reports |
| **Mobile Test Harnesses** | `test/build/android`, `test/build/ios`, `test/build/windowsphone` | JNI/NDK, Xcode, and Windows Phone native test wrappers |

---

## 2. Classification of Test Suites

The test codebase is organized into 6 main test categories:

```
openh264/test/
├── api/                        # [Category A] API, End-to-End & Conformance Tests
├── decoder/                    # [Category B] Decoder Algorithm Unit Tests
├── encoder/                    # [Category C] Encoder Core Unit Tests
├── processing/                 # [Category D] Video Preprocessing Unit Tests
├── common/                     # [Category E] Shared Data Structures & Threading Tests
└── encoder_binary_comparison/  # [Category F] Bit-Exact Regression & autotest/
```

---

### Category A: API & End-to-End Integration Tests (`test/api/`)
Validates public C/C++ API contracts, parameter validation, multithreaded decoding, error resilience, and end-to-end loopback fidelity.

- `test/api/c_interface_test.c` & `test/api/cpp_interface_test.cpp`: C and C++ API bindings, lifecycle, and memory management.
- `test/api/simple_test.cpp`: Sanity encode/decode flow.
- `test/api/encoder_test.cpp` & `test/api/decode_api_test.cpp`: Configuration options, parameter sets, and stride/buffer boundaries.
- `test/api/decoder_test.cpp`: Conformance bitstream decoding against reference frames.
- `test/api/decode_encode_test.cpp` & `test/api/encode_decode_api_test.cpp`: Full end-to-end loopback (Encode YUV -> NALUs -> Decode YUV -> verify fidelity via SHA-1 hashes).
- `test/api/decoder_ec_test.cpp`: Error concealment behavior under simulated packet loss or corrupted bitstreams.
- `test/api/encode_options_test.cpp`: Dynamic runtime parameter reconfiguration (bitrate, FPS, QP limits, spatial layer switches).
- `test/api/ltr_test.cpp`: Long-Term Reference (LTR) frame encoding and recovery.
- `test/api/thread_decoder_test.cpp`: Multi-threaded slice/frame decoder stability and race-condition checks.

---

### Category B: Decoder Core Unit Tests (`test/decoder/`)
Verifies isolated decoding pipeline stages against the H.264 standard:

- `test/decoder/DecUT_ParseSyntax.cpp`: NAL syntax, SPS/PPS header parsing, and slice headers.
- `test/decoder/DecUT_IntraPrediction.cpp`: Intra 4x4, 8x8, 16x16, and Chroma prediction modes (DC, Vertical, Horizontal, Plane).
- `test/decoder/DecUT_PredMv.cpp`: Motion vector derivation (median MV, spatial/temporal direct colocated MV).
- `test/decoder/DecUT_IdctResAddPred.cpp`: Inverse transforms (IDCT 4x4, Inverse Hadamard 4x4 / 2x2) and residual reconstruction.
- `test/decoder/DecUT_Deblock.cpp` & `test/decoder/DecUT_DeblockCommon.cpp`: Deblocking boundary strength (bS) and filter thresholds (alpha, beta).
- `test/decoder/DecUT_ErrorConcealment.cpp`: Frame/slice copy error concealment.
- `test/decoder/DecUT_DecExt.cpp`: Internal decoder memory allocation and extension handles.

---

### Category C: Encoder Core Unit Tests (`test/encoder/`)
Verifies algorithmic correctness, SIMD vs C reference equivalence, and SVC encoding logic:

- `test/encoder/EncUT_MotionEstimate.cpp`: Integer and fractional-pixel motion search, SAD/SATD cost calculation.
- `test/encoder/EncUT_MotionCompensation.cpp`: Half-pel / quarter-pel motion compensation block filtering.
- `test/encoder/EncUT_GetIntraPredictor.cpp`: Intra prediction mode selection in the encoder.
- `test/encoder/EncUT_Cavlc.cpp` & `test/encoder/EncUT_ExpGolomb.cpp`: Entropy bitstream generation (CAVLC / Exp-Golomb).
- `test/encoder/EncUT_EncoderMb.cpp`, `test/encoder/EncUT_EncoderMbAux.cpp`, `test/encoder/EncUT_DecodeMbAux.cpp`: Macroblock encoding, quantization, and transform loops.
- `test/encoder/EncUT_Reconstruct.cpp` & `test/encoder/EncUT_MBCopy.cpp`: Reconstructed reference picture copy and boundary management.
- `test/encoder/EncUT_SVC_me.cpp`: Inter-layer scalable video coding motion estimation.
- `test/encoder/EncUT_EncoderTaskManagement.cpp` & `test/encoder/EncUT_SliceBufferReallocate.cpp`: Multithreaded slice tasks and dynamic buffer reallocation.
- `test/encoder/EncUT_ParameterSetStrategy.cpp`, `test/encoder/EncUT_MemoryAlloc.cpp`, `test/encoder/EncUT_MemoryZero.cpp`: Memory buffers and parameter strategy.

---

### Category D: Video Preprocessing Unit Tests (`test/processing/`)
Tests image processing and frame feature extraction routines prior to macroblock encoding:

- `test/processing/ProcessUT_AdaptiveQuantization.cpp`: Spatial variance-based Adaptive Quantization (AQ).
- `test/processing/ProcessUT_DownSample.cpp`: Image downsampling filters for multi-resolution SVC layers.
- `test/processing/ProcessUT_ScrollDetection.cpp`: Screen-content scrolling motion detector.
- `test/processing/ProcessUT_VaaCalc.cpp`: Video Analysis and Adaptive (VAA) complexity statistics (SAD, frame variance).

---

### Category E: Common Infrastructure Unit Tests (`test/common/`)
- `test/common/CWelsListTest.cpp` & `test/common/WelsTaskListTest.cpp`: Internal lists and task queues.
- `test/common/WelsThreadPoolTest.cpp`: Worker thread lifecycle, mutual exclusion, and concurrent task dispatching.
- `test/common/ExpandPicture.cpp`: Frame border expansion for reference picture padding.

---

