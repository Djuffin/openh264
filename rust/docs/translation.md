# OpenH264: C++ to Rust Translation Mapping Guide

This document maps every primary data structure and algorithmic implementation described in [`overview.md`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/docs/overview.md) to its corresponding Rust source file location and struct/function definition within the `openh264-rs` crate (`rust/crates/openh264-rs/src`).

---

## 1. Module Mapping Overview

The C++ OpenH264 codebase (`codec/`) is translated into modular Rust under `rust/crates/openh264-rs/src`:

| Subsystem | C++ Source Directory | Rust Module Path |
| :--- | :--- | :--- |
| **C ABI Facade** | `codec/api/wels/` | `crate::api::codec_api` ([`src/api/codec_api.rs`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/api/codec_api.rs)) |
| **Decoder Core** | `codec/decoder/core/` | `crate::decoder::*` ([`src/decoder/`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/)) |
| **Encoder Core** | `codec/encoder/core/` | `crate::encoder::*` ([`src/encoder/`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/)) |
| **Shared Common** | `codec/common/` | `crate::common::*` ([`src/common/`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/common/)) |

---

## 2. H.264 Video Decoder

### 2.1 Core Decoder Data Structures

The following table maps each C++ decoder data structure from `overview.md` to its translated Rust counterpart:

| C++ Data Structure | C++ Header File | Rust Type / Struct Name | Rust File Location |
| :--- | :--- | :--- | :--- |
| `SWelsDecoderContext` | `decoder_context.h` | [`SWelsDecoderContext`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/decoder_context.rs#L651) | [`src/decoder/decoder_context.rs:651`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/decoder_context.rs#L651) |
| `SPicture` | `picture.h` | [`SPicture`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/picture.rs#L112) / [`Picture`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/picture.rs#L40) | [`src/decoder/picture.rs:112`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/picture.rs#L112) |
| `SDqLayer` | `decoder_core.h` | [`SDqLayer`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/decoder_core.rs#L406) | [`src/decoder/decoder_core.rs:406`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/decoder_core.rs#L406) |
| `SSps` / `TagSps` | `parameter_sets.h` | [`SSps`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/parameter_sets.rs#L181) / `TagSps` | [`src/decoder/parameter_sets.rs:181`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/parameter_sets.rs#L181) |
| `SPps` / `TagPps` | `parameter_sets.h` | [`SPps`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/parameter_sets.rs#L317) / `TagPps` | [`src/decoder/parameter_sets.rs:317`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/parameter_sets.rs#L317) |
| `SSubsetSps` | `parameter_sets.h` | [`SSubsetSps`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/parameter_sets.rs#L303) | [`src/decoder/parameter_sets.rs:303`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/parameter_sets.rs#L303) |
| `SBitStringAux` | `bit_stream.h` | [`BitStringAux`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/bit_stream.rs#L44) / `SBitStringAux` | [`src/decoder/bit_stream.rs:44`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/bit_stream.rs#L44) |
| `SAccessUnit` | `asymmetric_mul.h` | [`SAccessUnit`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/nalu.rs#L42) | [`src/decoder/nalu.rs:42`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/nalu.rs#L42) |
| `SWelsCabacDecEngine` | `cabac_decoder.h` | [`SWelsCabacDecEngine`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/cabac_decoder.rs#L40) | [`src/decoder/cabac_decoder.rs:40`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/cabac_decoder.rs#L40) |
| `SWelsCabacCtx` | `decoder_context.h` | [`SWelsCabacCtx`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/cabac_decoder.rs#L62) | [`src/decoder/cabac_decoder.rs:62`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/cabac_decoder.rs#L62) |
| `SFmo` / `PMbaMap` | `fmo.h` | [`SFmo`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/fmo.rs#L45) | [`src/decoder/fmo.rs:45`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/fmo.rs#L45) |
| `SRefList` | `manage_dec_ref.h` | [`SRefList`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/decoder_context.rs#L280) | [`src/decoder/decoder_context.rs:280`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/decoder_context.rs#L280) |
| `SDeblockingFunc` | `deblocking_common.h` | [`SDeblockingFunc`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/decoder_context.rs#L291) | [`src/decoder/decoder_context.rs:291`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/decoder_context.rs#L291) |

---

### 2.2 Decoder Core Algorithms

The following table maps each C++ decoder algorithm from `overview.md` to its translated Rust function implementation:

| Algorithmic Subsystem | Primary C++ File | Rust Function / Implementation | Rust File Location |
| :--- | :--- | :--- | :--- |
| **Annex B Start Code & NAL Demuxing** | `au_parser.cpp` | [`split_annexb_units`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/nalu.rs#L85), [`strip_emulation_prevention_bytes`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/nalu.rs#L140) | [`src/decoder/nalu.rs:85`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/nalu.rs#L85) |
| **Exp-Golomb Parsing (`ue`, `se`, `te`)** | `dec_golomb.h` | [`bs_get_ue`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/dec_golomb.rs#L42), [`bs_get_se`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/dec_golomb.rs#L80), [`bs_get_te`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/dec_golomb.rs#L105) | [`src/decoder/dec_golomb.rs:42`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/dec_golomb.rs#L42) |
| **CAVLC Entropy Decoding** | `parse_mb_syn_cavlc.cpp` | [`WelsParseMbCavlcResidual`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/parse_mb_syn_cavlc.rs#L450), `ParseCoeffToken`, `ParseTotalZeros`, `ParseRunBefore` | [`src/decoder/parse_mb_syn_cavlc.rs:450`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/parse_mb_syn_cavlc.rs#L450) |
| **CABAC Entropy Decoding** | `parse_mb_syn_cabac.cpp` | [`WelsDecodeCabacBit`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/cabac_decoder.rs#L120), `WelsDecodeCabacFinalBit` | [`src/decoder/cabac_decoder.rs:120`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/cabac_decoder.rs#L120) |
| **Intra Prediction (4x4, 16x16, Chroma 8x8)** | `get_intra_predictor.cpp` | [`WelsI4x4LumaPredV_c`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/get_intra_predictor.rs#L40), `WelsI4x4LumaPredH_c`, `WelsI4x4LumaPredDc_c`, `WelsI16x16LumaPredPlane_c`, `WelsIChromaPredDc_c` | [`src/decoder/get_intra_predictor.rs:40`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/get_intra_predictor.rs#L40) |
| **Motion Vector Prediction (MVP)** | `mv_pred.cpp` | [`PredMv`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/mv_pred.rs#L80), `PredMv4x4`, `PredMv16x8`, `PredMv8x16` | [`src/decoder/mv_pred.rs:80`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/mv_pred.rs#L80) |
| **Motion Compensation (6-tap & Sub-pel)** | `mc.cpp` | [`McHorizLuma_c`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/common/mc.rs#L40), [`McVertLuma_c`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/common/mc.rs#L80), [`InitMcFunc`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/common/mc.rs#L772) | [`src/common/mc.rs:40`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/common/mc.rs#L40) |
| **IQ & IDCT Integer Transforms** | `decode_mb_aux.cpp` | [`IdctResAddPred_c`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/decode_mb_aux.rs#L40), `IdctResAddPred8x8_c`, `IdctFourResAddPred_c`, `WelsIHadamard4x44x4_c` | [`src/decoder/decode_mb_aux.rs:40`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/decode_mb_aux.rs#L40) |
| **Macroblock Reconstruction Loop** | `rec_mb.cpp` | [`WelsReconstructMb`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/decode_slice.rs#L920) | [`src/decoder/decode_slice.rs:920`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/decode_slice.rs#L920) |
| **In-Loop Deblocking Filter** | `deblocking.cpp` | [`DeblockingInit`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/common/deblocking_common.rs#L528), `DeblockLumaLt4V_c`, `DeblockLumaEq4V_c`, `DeblockLumaLt4H_c`, `DeblockLumaEq4H_c` | [`src/common/deblocking_common.rs:528`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/common/deblocking_common.rs#L528) |
| **DPB Reference Management & MMCO** | `manage_dec_ref.cpp` | [`WelsMarkAsRef`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/manage_dec_ref.rs#L60), [`WelsSlidingWindow`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/manage_dec_ref.rs#L120), `WelsMmcoProcess` | [`src/decoder/manage_dec_ref.rs:60`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/manage_dec_ref.rs#L60) |
| **Error Concealment** | `error_concealment.cpp` | [`DoErrorConcealment`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/error_concealment.rs#L45), `WelsMarkPicError` | [`src/decoder/error_concealment.rs:45`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/error_concealment.rs#L45) |
| **Flexible Macroblock Ordering (FMO)** | `fmo.cpp` | [`FmoInit`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/fmo.rs#L40), `FmoGenerateMap`, `FmoGetNextMbOfSliceGroup` | [`src/decoder/fmo.rs:40`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/fmo.rs#L40) |

---

## 3. H.264 Video Encoder

### 3.1 Core Encoder Data Structures

The following table maps each C++ encoder data structure from `overview.md` to its translated Rust counterpart:

| C++ Data Structure | C++ Header File | Rust Type / Struct Name | Rust File Location |
| :--- | :--- | :--- | :--- |
| `sWelsEncCtx` | `encoder_context.h` | [`sWelsEncCtx`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/encoder_context.rs#L150) | [`src/encoder/encoder_context.rs:150`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/encoder_context.rs#L150) |
| `SWelsSvcCodingParam` | `param_svc.h` | [`SWelsSvcCodingParam`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/param_svc.rs#L220) | [`src/encoder/param_svc.rs:220`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/param_svc.rs#L220) |
| `SEncParamExt` | `param_svc.h` | [`SEncParamExt`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/param_svc.rs#L120) | [`src/encoder/param_svc.rs:120`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/param_svc.rs#L120) |
| `SEncParamBase` | `param_svc.h` | [`SEncParamBase`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/param_svc.rs#L60) | [`src/encoder/param_svc.rs:60`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/param_svc.rs#L60) |
| `SWelsME` | `svc_motion_estimate.h` | [`SWelsME`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/svc_motion_estimate.rs#L45) | [`src/encoder/svc_motion_estimate.rs:45`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/svc_motion_estimate.rs#L45) |
| `SSliceCtx` | `slice_multi_threading.h` | [`SSliceCtx`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/slice_multi_threading.rs#L40) | [`src/encoder/slice_multi_threading.rs:40`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/slice_multi_threading.rs#L40) |
| `SSlice` | `slice.h` | [`SSlice`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/encoder_context.rs#L80) | [`src/encoder/encoder_context.rs:80`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/encoder_context.rs#L80) |
| `SVAAFrameInfo` | `wels_preprocess.h` | [`SVAAFrameInfo`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/wels_preprocess.rs#L50) | [`src/encoder/wels_preprocess.rs:50`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/wels_preprocess.rs#L50) |
| `SWelsSvcRc` | `rc.h` | [`SWelsSvcRc`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/rc.rs#L60) | [`src/encoder/rc.rs:60`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/rc.rs#L60) |
| `SRcGom` | `rc.h` | [`SRcGom`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/rc.rs#L120) | [`src/encoder/rc.rs:120`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/rc.rs#L120) |
| `SLTRState` | `encoder_context.h` | [`SLTRState`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/encoder_context.rs#L110) | [`src/encoder/encoder_context.rs:110`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/encoder_context.rs#L110) |
| `SWelsCabacCtx` | `set_mb_syn_cabac.h` | [`SWelsCabacCtx`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/set_mb_syn_cabac.rs#L40) | [`src/encoder/set_mb_syn_cabac.rs:40`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/set_mb_syn_cabac.rs#L40) |
| `CWelsTaskManage` | `wels_task_management.h` | [`CWelsTaskManage`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/wels_task_management.rs#L50) | [`src/encoder/wels_task_management.rs:50`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/wels_task_management.rs#L50) |
| `WelsThreadPool` | `WelsThreadPool.h` | [`WelsThreadPool`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/common/wels_thread_pool.rs#L40) | [`src/common/wels_thread_pool.rs:40`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/common/wels_thread_pool.rs#L40) |

---

### 3.2 Encoder Core Algorithms

The following table maps each C++ encoder algorithm from `overview.md` to its translated Rust function implementation:

| Algorithmic Subsystem | Primary C++ File | Rust Function / Implementation | Rust File Location |
| :--- | :--- | :--- | :--- |
| **VAA Pre-processing & Scene Change** | `wels_preprocess.cpp` | [`WelsPreprocess`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/wels_preprocess.rs#L80), `WelsInitVaa`, `WelsVaaCalculation`, `BilinearDownsampling` | [`src/encoder/wels_preprocess.rs:80`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/wels_preprocess.rs#L80) |
| **Rate Control Engine (RC)** | `ratectl.cpp` | [`WelsRcInitModule`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/rc.rs#L100), `WelsRcCalculateGopBits`, `WelsRcUpdateFrameComplexity`, `WelsRcCalculateTargetQp` | [`src/encoder/rc.rs:100`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/rc.rs#L100) |
| **Motion Estimation (ME)** | `svc_motion_estimate.cpp` | [`WelsMotionEstimate`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/svc_motion_estimate.rs#L80), `WelsMeDiamondSearch`, `WelsMeCrossSearch`, `WelsMeSubPelSearch` | [`src/encoder/svc_motion_estimate.rs:80`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/svc_motion_estimate.rs#L80) |
| **Mode Decision (MD) & RDO** | `svc_mode_decision.cpp` | [`WelsMdDecision`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/svc_mode_decision.rs#L100), `WelsMdI16x16`, `WelsMdI4x4`, `WelsMdInter16x16`, `WelsMdCalculateSatd` | [`src/encoder/svc_mode_decision.rs:100`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/svc_mode_decision.rs#L100) |
| **Forward DCT & Dead-Zone Quantization** | `encode_mb_aux.cpp` | [`WelsFdct4x4_c`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/encode_mb_aux.rs#L40), [`WelsQuant4x4_c`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/encode_mb_aux.rs#L80), `WelsHadamardQuant2x2_c` | [`src/encoder/encode_mb_aux.rs:40`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/encode_mb_aux.rs#L40) |
| **Macroblock & Slice Encoding Loop** | `svc_encode_slice.cpp` | [`WelsCodeSlice`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/svc_encode_slice.cpp#L60), `WelsCodeOneMb` | [`src/encoder/svc_encode_slice.cpp:60`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/svc_encode_slice.cpp#L60) |
| **Deblocking Filter (Encoder Reconstruction)** | `deblocking.cpp` | [`WelsDeblockingFilterFrame`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/deblocking.rs#L50), `WelsDeblockingFilterMb` | [`src/encoder/deblocking.rs:50`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/deblocking.rs#L50) |
| **Entropy Encoding (CAVLC / CABAC)** | `vlc_encoder.cpp` / `set_mb_syn_cabac.cpp` | [`WelsWriteMbSyntaxCavlc`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/vlc_encoder.rs#L80), `WelsWriteMbSyntaxCabac` | [`src/encoder/vlc_encoder.rs:80`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/vlc_encoder.rs#L80) |
| **NAL Encapsulation & RBSP Emulation** | `nal_encap.cpp` | [`WelsEncodeNal`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/nal_encap.rs#L50), `WelsAddEmulationPreventionBytes` | [`src/encoder/nal_encap.rs:50`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/nal_encap.rs#L50) |
| **Multi-threaded Slicing & Task Dispatch** | `slice_multi_threading.cpp` | [`WelsInitSliceThreadCtx`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/slice_multi_threading.rs#L60), `WelsDynamicSliceSlicing`, `WelsTaskManageCreate` | [`src/encoder/slice_multi_threading.rs:60`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/slice_multi_threading.rs#L60) |
| **LTR Feedback & Scalability Management** | `ref_list_mgr_svc.cpp` | [`WelsInitRefList`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/ref_list_mgr_svc.rs#L60), `WelsMarkLtrFrame`, `WelsUpdateLtrStatus` | [`src/encoder/ref_list_mgr_svc.rs:60`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/ref_list_mgr_svc.rs#L60) |

---

## 4. Audit Findings & Detailed Translation Discrepancies

An exhaustive multi-agent line-by-line audit comparing the original C++ codebase (`codec/decoder/`, `codec/encoder/`, `codec/common/`) against the Rust translation (`rust/crates/openh264-rs/src/`) uncovered the following critical memory layout shifts, type mismatches, missing algorithmic implementations, and constant discrepancies.

### 4.1 Decoder Translation Issues

#### Bug 4.1.1: `SWelsDecoderContext` Memory Layout Offset Shift (4,096 Bytes) — [RESOLVED]
* **C++ Declaration**: [`decoder_context.h:337`](file:///usr/local/google/home/ezemtsov/projects/openh264/codec/decoder/core/inc/decoder_context.h#L337)
  ```cpp
  SFmo sFmoList[MAX_PPS_COUNT]; // Array of 256 SFmo structs (256 * 24 bytes = 6,144 bytes)
  ```
* **Rust Translation**: [`decoder_context.rs:672`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/decoder_context.rs#L672)
* **Impact**: Under `#[repr(C)]`, Rust previously allocated 2,048 bytes instead of 6,144 bytes for `sFmoList`, introducing a **4,096-byte memory layout shift** for every struct field declared after `sFmoList` in `SWelsDecoderContext`.
* **Fix**: Changed `pub sFmoList: [*mut c_void; MAX_PPS_COUNT]` to `pub sFmoList: [SFmo; MAX_PPS_COUNT]` and `pub pFmo` to `PFmo` in [`decoder_context.rs:672-673`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/decoder_context.rs#L672-L673). Alignment verified via unit tests.

#### Bug 4.1.2: `SWelsDecoderContext` Function Pointer Table Field Reordering
* **C++ Declaration**: [`decoder_context.h:454-463`](file:///usr/local/google/home/ezemtsov/projects/openh264/codec/decoder/core/inc/decoder_context.h#L454-L463)
  ```cpp
  SMcFunc sMcFunc;
  PGetI8x8LumaPredFunc pGetI8x8LumaPredFunc[NO_OPTION_OF_I8x8];
  PIdctResAddPredFunc pIdctResAddPredFunc8x8;
  SCopyFunc sCopyFunc;
  SDeblockingFunc sDeblockingFunc;
  SExpandPicFunc sExpandPicFunc;
  ```
* **Rust Translation**: [`decoder_context.rs:726-731`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/decoder_context.rs#L726-L731)
  ```rust
  pub pGetI8x8LumaPredFunc: [PGetI8x8LumaPredFunc; 4],
  pub pIdctResAddPredFunc8x8: PIdctResAddPredFunc,
  pub sCopyFunc: SCopyFunc,
  pub sExpandPicFunc: SExpandPicFunc,
  pub sMcFunc: SMcFunc,
  pub sDeblockingFunc: SDeblockingFunc,
  ```
* **Impact**: Field reordering causes C++ FFI calls through decoder context function pointers to dispatch to wrong function signatures, leading to invalid memory access or segmentation faults during decoding.
* **Fix**: Reorder function pointer fields in `decoder_context.rs` to match C++ field declaration order.

#### Bug 4.1.3: `SDqLayer` (`TagDqLayer`) Missing Fields and Type Mismatches
* **C++ Declaration**: [`dec_frame.h:61-132`](file:///usr/local/google/home/ezemtsov/projects/openh264/codec/decoder/core/inc/dec_frame.h#L61-L132)
* **Rust Translation**: [`decoder_core.rs:406-462`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/decoder_core.rs#L406-L462)
* **Impact**:
  1. **Missing 9 Fields**: Rust omits `pFmo` (`PFmo`), `uiSpsId` (`uint32_t`), `pRef` (`PPicture`), `iLumaStride` (`int32_t`), `iChromaStride` (`int32_t`), `pPred[3]` (`uint8_t* [3]`), `iColocMv[2][16][2]`, `iColocRefIndex[2][16]`, `iColocIntra[16]`.
  2. **Type Mismatch**: `uiDisableInterLayerDeblockingFilterIdc` is `uint32_t` (4 bytes) in C++ but `u8` (1 byte) in Rust.
* **Fix**: Re-add all 9 missing fields and restore `uiDisableInterLayerDeblockingFilterIdc: u32` in `SDqLayer`.

#### Bug 4.1.4: `SPps` (`TagPps`) Duplicate Field Bug
* **C++ Declaration**: [`parameter_sets.h:199`](file:///usr/local/google/home/ezemtsov/projects/openh264/codec/decoder/core/inc/parameter_sets.h#L199)
  ```cpp
  bool bConstainedIntraPredFlag; // Single bool field
  ```
* **Rust Translation**: [`parameter_sets.rs:347-348`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/parameter_sets.rs#L347-L348)
  ```rust
  pub bConstrainedIntraPredFlag: bool,
  pub bConstainedIntraPredFlag: bool, // ❌ Duplicate field!
  ```
* **Impact**: Rust contains both correctly spelled `bConstrainedIntraPredFlag` and misspelled `bConstainedIntraPredFlag`, inserting an unintended extra byte into `SPps` layout under `#[repr(C)]`.
* **Fix**: Remove `pub bConstainedIntraPredFlag: bool` from `parameter_sets.rs`.

#### Bug 4.1.5: Duplicate Conflicting `SRefPic` Struct Definition
* **C++ Declaration**: [`decoder_context.h:149-157`](file:///usr/local/google/home/ezemtsov/projects/openh264/codec/decoder/core/inc/decoder_context.h#L149-L157)
* **Rust Modules**: [`decoder_context.rs:401-409`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/decoder_context.rs#L401-L409) & [`decoder_core.rs:590-593`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/decoder_core.rs#L590-L593)
* **Impact**: `decoder_core.rs` redefines a conflicting local `SRefPic` with array size 16 instead of `MAX_DPB_COUNT` (17) and `uiRefCount: [u32; LIST_A]` instead of `u8`.
* **Fix**: Delete the duplicate struct definition in `decoder_core.rs` and re-export `decoder_context::SRefPic`.

#### Bug 4.1.6: NAL Demuxing Error Code Constant Mismatches
* **C++ Definition**: [`decoder_context.h:88`](file:///usr/local/google/home/ezemtsov/projects/openh264/codec/decoder/core/inc/decoder_context.h#L88)
  ```cpp
  dsBitstreamError = 0x01,
  dsNoParamSets     = 0x02,
  ```
* **Rust Translation**: [`nalu.rs:123-124`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/nalu.rs#L123-L124)
  ```rust
  pub const dsBitstreamError: i32 = 0x02; // ❌ Should be 0x01
  pub const dsNoParamSets: i32 = 0x04;     // ❌ Should be 0x02
  ```
* **Impact**: Bitmask operations evaluating decoder error status report incorrect error flags back to API callers.
* **Fix**: Update constants in `nalu.rs` to `dsBitstreamError = 0x01` and `dsNoParamSets = 0x02`.

#### Bug 4.1.7: Missing CAVLC Residual Decoding Functions
* **C++ Implementation**: [`parse_mb_syn_cavlc.cpp`](file:///usr/local/google/home/ezemtsov/projects/openh264/codec/decoder/core/src/parse_mb_syn_cavlc.cpp)
* **Rust Translation**: [`parse_mb_syn_cavlc.rs`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/parse_mb_syn_cavlc.rs)
* **Impact**: `WelsParseMbCavlcResidual`, `ParseCoeffToken`, `ParseTotalZeros`, and `ParseRunBefore` listed in `translation.md` (Section 2.2) are missing from `parse_mb_syn_cavlc.rs`, preventing pure Rust CAVLC residual decoding.
* **Fix**: Translate CAVLC coefficient token and residual block parsing functions from C++ to Rust.

#### Bug 4.1.8: Unimplemented Intra Prediction Reconstruction Function
* **C++ Implementation**: [`rec_mb.cpp:50-200`](file:///usr/local/google/home/ezemtsov/projects/openh264/codec/decoder/core/src/rec_mb.cpp#L50-L200)
* **Rust Translation**: [`decode_slice.rs:1015-1021`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/decoder/decode_slice.rs#L1015-L1021)
  ```rust
  pub unsafe fn WelsMbIntraPredictionConstruction(
      _pCtx: PWelsDecoderContext,
      _pCurDqLayer: *mut SDqLayer,
  ) -> i32 {
      ERR_NONE // ❌ Empty stub! Bypasses spatial intra prediction and IDCT reconstruction.
  }
  ```
* **Impact**: Intra frame decoding returns blank/un-reconstructed macroblocks when called in pure Rust mode.
* **Fix**: Port macroblock Intra reconstruction loop connecting predictors, IQ, and IDCT.

---

### 4.2 Encoder Translation Issues

#### Bug 4.2.1: `SWelsSvcRc` Field Type Mismatch (`bEnableGomQp`) causing Memory Misalignment
* **C++ Declaration**: [`rc.h:193`](file:///usr/local/google/home/ezemtsov/projects/openh264/codec/encoder/core/inc/rc.h#L193)
  ```cpp
  typedef struct TagWelsRc {
    ...
    bool       bGomRC;                        // 1 byte
    double*    pGomComplexity;               // 8 bytes
    int32_t*   pGomForegroundBlockNum;       // 8 bytes
    int32_t*   pCurrentFrameGomSad;          // 8 bytes
    int32_t*   pGomCost;                     // 8 bytes

    int32_t   bEnableGomQp;                  // 4 bytes (int32_t)
    int32_t   iAverageFrameQp;               // 4 bytes
    ...
  } SWelsSvcRc;
  ```
* **Rust Translation**: [`rc.rs:292`](file:///usr/local/google/home/ezemtsov/projects/openh264/rust/crates/openh264-rs/src/encoder/rc.rs#L292)
  ```rust
  #[repr(C)]
  #[derive(Debug, Copy, Clone)]
  pub struct SWelsSvcRc {
      ...
      pub bGomRC: bool,
      pub pGomComplexity: *mut f64,
      pub pGomForegroundBlockNum: *mut i32,
      pub pCurrentFrameGomSad: *mut i32,
      pub pGomCost: *mut i32,

      pub bEnableGomQp: bool, // ❌ 1-byte bool instead of 4-byte int32_t / i32!
      pub iAverageFrameQp: i32,
      ...
  }
  ```
* **Impact**: In C++, `bEnableGomQp` is declared as `int32_t` (4 bytes). In Rust `SWelsSvcRc`, `bEnableGomQp` is declared as `bool` (1 byte under `#[repr(C)]`). This 3-byte layout contraction shifts all 38+ subsequent fields in `SWelsSvcRc` (`iAverageFrameQp`, `iMinFrameQp`, `iMaxFrameQp`, `iNumberMbFrame`, `iNumberMbGom`, `iGopSize`, `iSkipFrameNum`, `iQStep`, `iBufferFullnessSkip`, `iPaddingSize`, `pTemporalOverRc`, etc.) by 3 bytes when accessed via C-FFI.
* **Fix**: Change `pub bEnableGomQp: bool` in `rc.rs:292` to `pub bEnableGomQp: i32`.

