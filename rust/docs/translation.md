# OpenH264: C++ to Rust Translation Mapping Guide

This document maps every primary data structure and algorithmic implementation described in [`overview.md`](rust/docs/overview.md) to its corresponding Rust source file location and struct/function definition within the `openh264-rs` crate (`rust/crates/openh264-rs/src`).

---

## 1. Module Mapping Overview

The C++ OpenH264 codebase (`codec/`) is translated into modular Rust under `rust/crates/openh264-rs/src`:

| Subsystem | C++ Source Directory | Rust Module Path |
| :--- | :--- | :--- |
| **C ABI Facade** | `codec/api/wels/` | `crate::api::codec_api` ([`src/api/codec_api.rs`](rust/crates/openh264-rs/src/api/codec_api.rs)) |
| **Decoder Core** | `codec/decoder/core/` | `crate::decoder::*` ([`src/decoder/`](rust/crates/openh264-rs/src/decoder/)) |
| **Encoder Core** | `codec/encoder/core/` | `crate::encoder::*` ([`src/encoder/`](rust/crates/openh264-rs/src/encoder/)) |
| **Shared Common** | `codec/common/` | `crate::common::*` ([`src/common/`](rust/crates/openh264-rs/src/common/)) |

---

## 2. Literate Walkthrough Mapping: C++ Baseline Decoder to Rust (`openh264-rs`)

This section maps every C++ function described in the baseline decoding literate walkthrough ([`decoding_process.md`](decoding_process.md)) to its translated Rust counterpart in `rust/crates/openh264-rs/src/`.

### 2.1 Missing Rust Functions Highlight `[RESOLVED]`

The following C++ functions from `decoding_process.md`, which previously lacked a standalone 1-to-1 Rust function counterpart in `openh264-rs`, have been fully translated and integrated:

1. **`WelsDecodeBs`** (C++ [`decoder.cpp:741`](codec/decoder/core/src/decoder.cpp#L741)):
   - **Status**: **RESOLVED**
   - **Rust Implementation**: `pub unsafe fn WelsDecodeBs` in [`src/decoder/decoder_core.rs:2877`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L2877)
   - **Explanation**: Extracted the Annex B start-code scanning and NAL unit demuxing loop from `decoder_decode_frame2_c` into a standalone `WelsDecodeBs` function in `decoder_core.rs`, matching the C++ baseline decoding flow.
2. **`WelsOpenDecoder`** (C++ [`decoder.cpp:52`](codec/decoder/core/src/decoder.cpp#L52)):
   - **Status**: **RESOLVED**
   - **Rust Implementation**: `pub unsafe fn WelsOpenDecoder` in [`src/decoder/decoder_core.rs:1939`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L1939)
   - **Explanation**: Implemented standalone `WelsOpenDecoder` that performs CPU feature detection (`WelsCPUFeatureDetect`), thread initialization (`OpenDecoderThreads`), and function pointer binding (`WelsInitDecoderFuncs`), invoked during static memory initialization.
3. **`WelsEndDecoder`** (C++ [`decoder.cpp:711`](codec/decoder/core/src/decoder.cpp#L711)):
   - **Status**: **RESOLVED**
   - **Rust Implementation**: `pub unsafe fn WelsEndDecoder` in [`src/decoder/decoder_core.rs:1952`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L1952)
   - **Explanation**: Implemented standalone `WelsEndDecoder` that calls `CloseDecoderThreads` and `WelsFreeStaticMemory`, invoked during decoder uninitialization.
4. **`CWelsDecoder::OpenDecoderThreads` / `CloseDecoderThreads` / `WelsTaskThread`** (C++ [`welsDecoderExt.cpp`](codec/decoder/plus/src/welsDecoderExt.cpp)):
   - **Status**: **RESOLVED**
   - **Rust Implementation**: `OpenDecoderThreads`, `CloseDecoderThreads`, `WelsTaskThread` in [`src/decoder/decoder_core.rs:1916-1937`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L1916-L1937)
   - **Explanation**: Implemented explicit decoder thread lifecycle hooks and task thread worker callbacks for single-threaded synchronous execution.
5. **`WelsCPUFeatureDetect` / `GetCPUCount`** (C++ [`decoder.cpp`](codec/decoder/core/src/decoder.cpp)):
   - **Status**: **RESOLVED**
   - **Rust Implementation**: `GetCPUCount`, `WelsCPUFeatureDetect` in [`src/decoder/decoder_core.rs:1901-1914`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L1901-L1914)
   - **Explanation**: Implemented CPU core detection and SIMD feature flag detection routines matching OpenH264 C++ behavior.

---

### 2.2 Complete Chapter-by-Chapter C++ to Rust Walkthrough Table

#### Chapter 1: Decoder Instantiation & Initialization
| C++ Function in `decoding_process.md` | Corresponding Rust Function / Implementation | Rust Source File & Line Location |
| :--- | :--- | :--- |
| `WelsCreateDecoder(ISVCDecoder**)` | `pub unsafe extern "C" fn WelsCreateDecoder` | [`src/api/codec_api.rs:1668`](rust/crates/openh264-rs/src/api/codec_api.rs#L1668) |
| `CWelsDecoder::CWelsDecoder()` | `CWelsDecoderImpl` struct initialization | [`src/api/codec_api.rs:1684`](rust/crates/openh264-rs/src/api/codec_api.rs#L1684) |
| `CWelsDecoder::Initialize(SDecodingParam*)` | `decoder_initialize_c` | [`src/api/codec_api.rs:1403`](rust/crates/openh264-rs/src/api/codec_api.rs#L1403) |
| `CWelsDecoder::InitDecoder(SDecodingParam*)` | `decoder_initialize_c` | [`src/api/codec_api.rs:1403`](rust/crates/openh264-rs/src/api/codec_api.rs#L1403) |
| `CWelsDecoder::InitDecoderCtx(...)` / `WelsInitDecoder` | `WelsInitStaticMemory` + `WelsInitDecoderFuncs` | [`src/decoder/decoder_core.rs:1826, 1899`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L1826) |
| `WelsOpenDecoder(PWelsDecoderContext, ...)` | `pub unsafe fn WelsOpenDecoder` | [`src/decoder/decoder_core.rs:1939`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L1939) |
| `WelsInitStaticMemory(PWelsDecoderContext)` | `pub unsafe fn WelsInitStaticMemory` | [`src/decoder/decoder_core.rs:1899`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L1899) |
| `InitDecFuncs` / `InitPredFunc` | `WelsInitDecoderFuncs`, `DeblockingInit`, `InitMcFunc` | [`src/decoder/decoder_core.rs:1826`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L1826), [`src/common/deblocking_common.rs:528`](rust/crates/openh264-rs/src/common/deblocking_common.rs#L528), [`src/common/mc.rs:772`](rust/crates/openh264-rs/src/common/mc.rs#L772) |
| `WelsBlockFuncInit(TagBlockFunc*, int32_t)` | `pub unsafe fn WelsBlockFuncInit` | [`src/decoder/decode_slice.rs:900`](rust/crates/openh264-rs/src/decoder/decode_slice.rs#L900) |

#### Chapter 2: Bitstream Ingestion & NAL Unit Demuxing
| C++ Function in `decoding_process.md` | Corresponding Rust Function / Implementation | Rust Source File & Line Location |
| :--- | :--- | :--- |
| `CWelsDecoder::DecodeFrame2(...)` | `decoder_decode_frame2_c` | [`src/api/codec_api.rs:1478`](rust/crates/openh264-rs/src/api/codec_api.rs#L1478) |
| `WelsDecodeBs(PWelsDecoderContext, ...)` | `pub unsafe fn WelsDecodeBs` | [`src/decoder/decoder_core.rs:2877`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L2877) |
| `ParseNalHeader(...)` | `pub unsafe fn ParseNalHeader` | [`src/decoder/nalu.rs:260`](rust/crates/openh264-rs/src/decoder/nalu.rs#L260) |
| `ParseNonVclNal(...)` | `pub unsafe fn ParseNonVclNal` | [`src/decoder/nalu.rs:955`](rust/crates/openh264-rs/src/decoder/nalu.rs#L955) |
| `ParseSps(...)` | `pub unsafe fn ParseSps` | [`src/decoder/nalu.rs:1265`](rust/crates/openh264-rs/src/decoder/nalu.rs#L1265) |
| `ParsePps(...)` | `pub unsafe fn ParsePps` | [`src/decoder/nalu.rs:1542`](rust/crates/openh264-rs/src/decoder/nalu.rs#L1542) |
| `DecInitBits` / `InitReadBits` | `pub unsafe fn DecInitBits`, `pub unsafe fn InitReadBits` | [`src/decoder/bit_stream.rs:102, 129`](rust/crates/openh264-rs/src/decoder/bit_stream.rs#L102) |
| `BsGetBits`, `BsGetUe`, `BsGetSe` | `BsGetBits`, `BsGetUe`, `BsGetSe` | [`src/decoder/dec_golomb.rs:157, 233, 275`](rust/crates/openh264-rs/src/decoder/dec_golomb.rs#L157) |

#### Chapter 3: Slice Header Parsing & DPB Reference Management
| C++ Function in `decoding_process.md` | Corresponding Rust Function / Implementation | Rust Source File & Line Location |
| :--- | :--- | :--- |
| `ParseSliceHeaderSyntaxs(...)` | `pub unsafe fn ParseSliceHeaderSyntaxs` | [`src/decoder/decoder_core.rs:1997`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L1997) |
| `FillDefaultSliceHeaderExt(...)` | `pub unsafe fn FillDefaultSliceHeaderExt` | [`src/decoder/decoder_core.rs:1625`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L1625) |
| `ParseRefPicListReordering(...)` | `pub unsafe fn ParseRefPicListReordering` | [`src/decoder/decoder_core.rs:1452`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L1452) |
| `ParseDecRefPicMarking(...)` | `pub unsafe fn ParseDecRefPicMarking` | [`src/decoder/decoder_core.rs:1521`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L1521) |
| `DecodeCurrentAccessUnit(...)` | `pub unsafe fn DecodeCurrentAccessUnit` | [`src/decoder/decoder_core.rs:2940`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L2940) |
| `WelsDecodeAccessUnitStart(PWelsDecoderContext)` | `pub unsafe fn WelsDecodeAccessUnitStart` | [`src/decoder/decoder_core.rs:2569`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L2569) |
| `AllocPicBuffOnNewSeqBegin(PWelsDecoderContext)` | `pub unsafe fn AllocPicBuffOnNewSeqBegin` | [`src/decoder/decoder_core.rs:2744`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L2744) |
| `PrefetchPic(TagPicBuff*)` | `pub unsafe fn PrefetchPic` | [`src/decoder/pic_queue.rs:429`](rust/crates/openh264-rs/src/decoder/pic_queue.rs#L429) |
| `InitRefPicList(...)` / `WelsInitRefList(...)` | `pub unsafe fn WelsInitRefList` | [`src/decoder/manage_dec_ref.rs:966`](rust/crates/openh264-rs/src/decoder/manage_dec_ref.rs#L966) |
| `WelsReorderRefList(PWelsDecoderContext)` | `pub unsafe fn WelsReorderRefList` | [`src/decoder/manage_dec_ref.rs:1140`](rust/crates/openh264-rs/src/decoder/manage_dec_ref.rs#L1140) |
| `WelsDqLayerDecodeStart(...)` | `pub unsafe fn WelsDqLayerDecodeStart` | [`src/decoder/decoder_core.rs:2869`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L2869) |

#### Chapter 4: Macroblock Entropy Decoding (CAVLC)
| C++ Function in `decoding_process.md` | Corresponding Rust Function / Implementation | Rust Source File & Line Location |
| :--- | :--- | :--- |
| `WelsDecodeSlice(...)` | `pub unsafe fn WelsDecodeSlice` | [`src/decoder/decode_slice.rs:2705`](rust/crates/openh264-rs/src/decoder/decode_slice.rs#L2705) |
| `WelsDecodeMbCavlcISlice(...)` | `WelsDecodeMbCavlcISlice`, `WelsActualDecodeMbCavlcISlice` | [`src/decoder/decode_slice.rs:1378, 1382`](rust/crates/openh264-rs/src/decoder/decode_slice.rs#L1378) |
| `ParseIntra4x4Mode(...)` | `pub unsafe fn ParseIntra4x4Mode` | [`src/decoder/decode_slice.rs:1435`](rust/crates/openh264-rs/src/decoder/decode_slice.rs#L1435) |
| `ParseIntra16x16Mode(...)` | `pub unsafe fn ParseIntra16x16Mode` | [`src/decoder/decode_slice.rs:1688`](rust/crates/openh264-rs/src/decoder/decode_slice.rs#L1688) |
| `WelsDecodeMbCavlcPSlice(...)` | `WelsDecodeMbCavlcPSlice`, `WelsActualDecodeMbCavlcPSlice` | [`src/decoder/decode_slice.rs:1397, 1401`](rust/crates/openh264-rs/src/decoder/decode_slice.rs#L1397) |
| `PredPSkipMvFromNeighbor(...)` | `pub unsafe fn PredPSkipMvFromNeighbor` | [`src/decoder/mv_pred.rs:564`](rust/crates/openh264-rs/src/decoder/mv_pred.rs#L564) |
| `WelsResidualBlockCavlc(...)` | `pub unsafe fn WelsResidualBlockCavlc` | [`src/decoder/parse_mb_syn_cavlc.rs:1617`](rust/crates/openh264-rs/src/decoder/parse_mb_syn_cavlc.rs#L1617) |
| `WelsCalcDeqCoeffScalingList(PWelsDecoderContext)` | `pub unsafe fn WelsCalcDeqCoeffScalingList` | [`src/decoder/decode_slice.rs:669`](rust/crates/openh264-rs/src/decoder/decode_slice.rs#L669) |

#### Chapter 5: Macroblock Reconstruction
| C++ Function in `decoding_process.md` | Corresponding Rust Function / Implementation | Rust Source File & Line Location |
| :--- | :--- | :--- |
| `WelsTargetMbConstruction(PWelsDecoderContext)` | `pub unsafe fn WelsTargetMbConstruction` | [`src/decoder/decode_slice.rs:1254`](rust/crates/openh264-rs/src/decoder/decode_slice.rs#L1254) |
| `WelsFillRecNeededMbInfo(...)` | `pub unsafe fn WelsFillRecNeededMbInfo` | [`src/decoder/decode_slice.rs:1015`](rust/crates/openh264-rs/src/decoder/decode_slice.rs#L1015) |
| `WelsMbIntraPredictionConstruction(...)` | `pub unsafe fn WelsMbIntraPredictionConstruction` | [`src/decoder/decode_slice.rs:1225`](rust/crates/openh264-rs/src/decoder/decode_slice.rs#L1225) |
| `RecI16x16Mb(...)` | `pub unsafe fn RecI16x16Mb` | [`src/decoder/decode_slice.rs:1190`](rust/crates/openh264-rs/src/decoder/decode_slice.rs#L1190) |
| `RecI4x4Mb(...)` / `RecI4x4Luma()` | `pub unsafe fn RecI4x4Mb` | [`src/decoder/decode_slice.rs:1117`](rust/crates/openh264-rs/src/decoder/decode_slice.rs#L1117) |
| `RecChroma(...)` | `pub unsafe fn RecChroma` | [`src/decoder/decode_slice.rs:1042`](rust/crates/openh264-rs/src/decoder/decode_slice.rs#L1042) |
| `WelsLumaDcDequantIdct` / `WelsChromaDcIdct` | `WelsLumaDcDequantIdct`, `WelsChromaDcIdct` | [`src/decoder/decode_slice.rs:689, 738`](rust/crates/openh264-rs/src/decoder/decode_slice.rs#L689), [`src/decoder/parse_mb_syn_cavlc.rs:1240, 1314`](rust/crates/openh264-rs/src/decoder/parse_mb_syn_cavlc.rs#L1240) |
| `WelsMbInterPrediction` / `WelsMbInterConstruction` | `pub unsafe fn WelsMbInterPrediction`, `pub unsafe fn WelsMbInterConstruction` | [`src/decoder/decode_slice.rs:976, 1008`](rust/crates/openh264-rs/src/decoder/decode_slice.rs#L976) |
| `PredMv(...)` | `pub unsafe fn PredMv` | [`src/decoder/mv_pred.rs:729`](rust/crates/openh264-rs/src/decoder/mv_pred.rs#L729) |
| `UpdateP16x16MotionInfo`, `UpdateP16x8MotionInfo`, `UpdateP8x16MotionInfo` | `UpdateP16x16MotionInfo`, `UpdateP16x8MotionInfo`, `UpdateP8x16MotionInfo` | [`src/decoder/mv_pred.rs:1348, 1439, 1500`](rust/crates/openh264-rs/src/decoder/mv_pred.rs#L1348) |
| `WelsMbInterSampleConstruction(...)` / `WelsDec::GetInterPred` | `pub unsafe fn WelsMbInterSampleConstruction` | [`src/decoder/decode_slice.rs:913`](rust/crates/openh264-rs/src/decoder/decode_slice.rs#L913) |
| `mc_luma` / `mc_chroma` / `mb_copy` | `McHorizLuma_c`, `McVertLuma_c`, `McChroma_c`, `McCopy_c` | [`src/common/mc.rs:40-250`](rust/crates/openh264-rs/src/common/mc.rs#L40-L250) |
| `IdctResAddPred_c(...)` | `pub unsafe extern "C" fn IdctResAddPred_c` | [`src/decoder/decode_mb_aux.rs:133`](rust/crates/openh264-rs/src/decoder/decode_mb_aux.rs#L133) |
| `WelsTargetSliceConstruction(PWelsDecoderContext)` | `pub unsafe fn WelsTargetSliceConstruction` | [`src/decoder/decode_slice.rs:1290`](rust/crates/openh264-rs/src/decoder/decode_slice.rs#L1290) |

#### Chapter 6: Deblocking Filter (In-Loop Filter)
| C++ Function in `decoding_process.md` | Corresponding Rust Function / Implementation | Rust Source File & Line Location |
| :--- | :--- | :--- |
| `WelsDeblockingFilterSlice(...)` | `pub unsafe fn WelsDeblockingFilterSlice` | [`src/decoder/deblocking.rs:2315`](rust/crates/openh264-rs/src/decoder/deblocking.rs#L2315) |
| `WelsDeblockingMb(...)` / `DeblockingInterMb(...)` | `WelsDeblockingMb`, `DeblockingInterMb` | [`src/decoder/deblocking.rs:1805, 2212`](rust/crates/openh264-rs/src/decoder/deblocking.rs#L1805) |
| `DeblockingBsMarginalMBAvcbase(...)` / `DeblockingBSInsideMBNormal(...)` | `DeblockingBsMarginalMBAvcbase`, `DeblockingBSInsideMBNormal` | [`src/decoder/deblocking.rs:716, 997`](rust/crates/openh264-rs/src/decoder/deblocking.rs#L716) |
| `FilteringEdgeLumaHV(...)` / `FilteringEdgeChromaHV(...)` | `FilteringEdgeLumaHV`, `FilteringEdgeChromaHV` | [`src/decoder/deblocking.rs:1946, 2055`](rust/crates/openh264-rs/src/decoder/deblocking.rs#L1946) |
| Low-level edge routines (`FilteringEdgeLumaIntraV`, `FilteringEdgeLumaV`, etc.) | `FilteringEdgeLumaIntraV`, `FilteringEdgeLumaV`, `FilteringEdgeChromaV`, etc. | [`src/decoder/deblocking.rs:1040-1800`](rust/crates/openh264-rs/src/decoder/deblocking.rs#L1040-L1800) |

#### Chapter 7: Frame Finalization, DPB Management & Output Export
| C++ Function in `decoding_process.md` | Corresponding Rust Function / Implementation | Rust Source File & Line Location |
| :--- | :--- | :--- |
| `WelsDecodeAccessUnitEnd(PWelsDecoderContext)` | `pub unsafe fn WelsDecodeAccessUnitEnd` | [`src/decoder/decoder_core.rs:2585`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L2585) |
| `WelsMarkAsRef(PWelsDecoderContext, PPicture)` | `pub unsafe fn WelsMarkAsRef` | [`src/decoder/manage_dec_ref.rs:1415`](rust/crates/openh264-rs/src/decoder/manage_dec_ref.rs#L1415) |
| `SlidingWindow(PWelsDecoderContext, TagRefPic*)` | `pub unsafe fn SlidingWindow` | [`src/decoder/manage_dec_ref.rs:578`](rust/crates/openh264-rs/src/decoder/manage_dec_ref.rs#L578) |
| `CheckAndFinishLastPic(...)` | `pub unsafe fn CheckAndFinishLastPic` | [`src/decoder/decoder_core.rs:3193`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L3193) |
| `DecodeFrameConstruction(...)` | `pub unsafe fn DecodeFrameConstruction` | [`src/decoder/decoder_core.rs:1048`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L1048) |
| `UpdateDecoderStatisticsForActiveParaset(...)` / `UpdateDecStat(...)` | `UpdateDecoderStatisticsForActiveParaset`, `UpdateDecStat` | [`src/decoder/decoder_core.rs:830, 1981`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L830) |

#### Chapter 8: Teardown & Destruction
| C++ Function in `decoding_process.md` | Corresponding Rust Function / Implementation | Rust Source File & Line Location |
| :--- | :--- | :--- |
| `CWelsDecoder::Uninitialize()` / `UninitDecoder()` | `decoder_uninitialize_c`, `decoder_uninit_c` | [`src/api/codec_api.rs:1419, 1425`](rust/crates/openh264-rs/src/api/codec_api.rs#L1419) |
| `CWelsDecoder::CloseDecoderThreads()` / `WelsEndDecoder(...)` | `CloseDecoderThreads`, `pub unsafe fn WelsEndDecoder` | [`src/decoder/decoder_core.rs:1931, 1952`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L1931) |
| `CWelsDecoder::UninitDecoderCtx(...)` / `WelsFreeStaticMemory(...)` | `pub unsafe fn WelsFreeStaticMemory` | [`src/decoder/decoder_core.rs:1924`](rust/crates/openh264-rs/src/decoder/decoder_core.rs#L1924) |
| `WelsDestroyDecoder(ISVCDecoder*)` | `pub unsafe extern "C" fn WelsDestroyDecoder` | [`src/api/codec_api.rs:1718`](rust/crates/openh264-rs/src/api/codec_api.rs#L1718) |


