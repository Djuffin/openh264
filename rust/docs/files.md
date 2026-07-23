# OpenH264: Source Code File Inventory

This document provides a comprehensive inventory of all source and header files referenced in [`overview.md`](overview.md), categorized by subsystem and module.

---

## 1. Decoder Subsystem (`codec/decoder/`)

### Header Files (`.h`)
* [`codec/decoder/plus/inc/welsDecoderExt.h`](../../codec/decoder/plus/inc/welsDecoderExt.h) — Public C++ facade wrapper header
* [`codec/decoder/core/inc/decoder_context.h`](../../codec/decoder/core/inc/decoder_context.h) — Core decoder context structure (`SWelsDecoderContext`)
* [`codec/decoder/core/inc/decoder_core.h`](../../codec/decoder/core/inc/decoder_core.h) — Decoder core definitions & dependency layer structures
* [`codec/decoder/core/inc/nalu.h`](../../codec/decoder/core/inc/nalu.h) — NAL unit definitions and bitstream extraction types
* [`codec/decoder/core/inc/parameter_sets.h`](../../codec/decoder/core/inc/parameter_sets.h) — Sequence (`SSps`) & Picture (`SPps`) parameter sets
* [`codec/decoder/core/inc/bit_stream.h`](../../codec/decoder/core/inc/bit_stream.h) — Bitstream reader helper structures (`SBitStringAux`)
* [`codec/decoder/core/inc/dec_golomb.h`](../../codec/decoder/core/inc/dec_golomb.h) — Exp-Golomb bitstream parser routines
* [`codec/decoder/core/inc/cabac_decoder.h`](../../codec/decoder/core/inc/cabac_decoder.h) — CABAC entropy arithmetic decoder definitions
* [`codec/decoder/core/inc/decode_slice.h`](../../codec/decoder/core/inc/decode_slice.h) — Slice decoding prototypes
* [`codec/decoder/core/inc/slice.h`](../../codec/decoder/core/inc/slice.h) — Slice header structures (`SSliceHeader`)
* [`codec/decoder/core/inc/get_intra_predictor.h`](../../codec/decoder/core/inc/get_intra_predictor.h) — Spatial intra prediction declarations
* [`codec/decoder/core/inc/mv_pred.h`](../../codec/decoder/core/inc/mv_pred.h) — Motion vector prediction headers
* [`codec/decoder/core/inc/decode_mb_aux.h`](../../codec/decoder/core/inc/decode_mb_aux.h) — Inverse quantization and IDCT routines
* [`codec/decoder/core/inc/deblocking.h`](../../codec/decoder/core/inc/deblocking.h) — In-loop deblocking filter declarations
* [`codec/decoder/core/inc/manage_dec_ref.h`](../../codec/decoder/core/inc/manage_dec_ref.h) — DPB reference frame list management
* [`codec/decoder/core/inc/pic_queue.h`](../../codec/decoder/core/inc/pic_queue.h) — Reconstructed picture buffer pool (`PPicBuff`)
* [`codec/decoder/core/inc/picture.h`](../../codec/decoder/core/inc/picture.h) — Reconstructed picture buffer definitions (`PPicture`)
* [`codec/decoder/core/inc/error_concealment.h`](../../codec/decoder/core/inc/error_concealment.h) — Error concealment & resilience declarations
* [`codec/decoder/core/inc/fmo.h`](../../codec/decoder/core/inc/fmo.h) — Flexible Macroblock Ordering declarations

### Implementation Files (`.cpp`)
* [`codec/decoder/plus/src/welsDecoderExt.cpp`](../../codec/decoder/plus/src/welsDecoderExt.cpp) — Public C++ facade wrapper implementation
* [`codec/decoder/core/src/decoder.cpp`](../../codec/decoder/core/src/decoder.cpp) — Top-level decoder API entry points
* [`codec/decoder/core/src/decoder_core.cpp`](../../codec/decoder/core/src/decoder_core.cpp) — Decoder core setup & initialization
* [`codec/decoder/core/src/au_parser.cpp`](../../codec/decoder/core/src/au_parser.cpp) — Annex B NAL unit parser and RBSP un-escaping
* [`codec/decoder/core/src/bit_stream.cpp`](../../codec/decoder/core/src/bit_stream.cpp) — Bitstream reading functions
* [`codec/decoder/core/src/parse_mb_syn_cavlc.cpp`](../../codec/decoder/core/src/parse_mb_syn_cavlc.cpp) — CAVLC entropy decoding routines
* [`codec/decoder/core/src/parse_mb_syn_cabac.cpp`](../../codec/decoder/core/src/parse_mb_syn_cabac.cpp) — CABAC syntax parsing functions
* [`codec/decoder/core/src/cabac_decoder.cpp`](../../codec/decoder/core/src/cabac_decoder.cpp) — CABAC arithmetic decoding engine
* [`codec/decoder/core/src/decode_slice.cpp`](../../codec/decoder/core/src/decode_slice.cpp) — Macroblock slice decoding pipeline
* [`codec/decoder/core/src/rec_mb.cpp`](../../codec/decoder/core/src/rec_mb.cpp) — Macroblock reconstruction loop
* [`codec/decoder/core/src/get_intra_predictor.cpp`](../../codec/decoder/core/src/get_intra_predictor.cpp) — Intra 4x4, 16x16, and chroma prediction
* [`codec/decoder/core/src/mv_pred.cpp`](../../codec/decoder/core/src/mv_pred.cpp) — Motion vector prediction calculation
* [`codec/decoder/core/src/decode_mb_aux.cpp`](../../codec/decoder/core/src/decode_mb_aux.cpp) — Inverse transform (4x4 IDCT / Hadamard)
* [`codec/decoder/core/src/deblocking.cpp`](../../codec/decoder/core/src/deblocking.cpp) — Boundary strength & edge deblocking filter
* [`codec/decoder/core/src/manage_dec_ref.cpp`](../../codec/decoder/core/src/manage_dec_ref.cpp) — Reference list construction and MMCO logic
* [`codec/decoder/core/src/pic_queue.cpp`](../../codec/decoder/core/src/pic_queue.cpp) — Picture buffer management
* [`codec/decoder/core/src/error_concealment.cpp`](../../codec/decoder/core/src/error_concealment.cpp) — Spatial & temporal error concealment
* [`codec/decoder/core/src/fmo.cpp`](../../codec/decoder/core/src/fmo.cpp) — Flexible Macroblock Ordering parsing

---

## 2. Encoder Subsystem (`codec/encoder/`)

### Header Files (`.h`)
* [`codec/encoder/plus/inc/welsEncoderExt.h`](../../codec/encoder/plus/inc/welsEncoderExt.h) — Public C++ facade wrapper header
* [`codec/encoder/core/inc/encoder_context.h`](../../codec/encoder/core/inc/encoder_context.h) — Core encoder context (`sWelsEncCtx`) and LTR state (`SLTRState`)
* [`codec/encoder/core/inc/param_svc.h`](../../codec/encoder/core/inc/param_svc.h) — SVC encoding parameters (`SWelsSvcCodingParam`)
* [`codec/encoder/core/inc/wels_preprocess.h`](../../codec/encoder/core/inc/wels_preprocess.h) — Video pre-processing & complexity assessment (VAA)
* [`codec/encoder/core/inc/rc.h`](../../codec/encoder/core/inc/rc.h) — Rate control engine definitions (`SWelsSvcRc`)
* [`codec/encoder/core/inc/svc_motion_estimate.h`](../../codec/encoder/core/inc/svc_motion_estimate.h) — Motion estimation structures (`SWelsME`)
* [`codec/encoder/core/inc/svc_mode_decision.h`](../../codec/encoder/core/inc/svc_mode_decision.h) — SVC mode decision interfaces
* [`codec/encoder/core/inc/md.h`](../../codec/encoder/core/inc/md.h) — Rate-distortion mode decision structures
* [`codec/encoder/core/inc/svc_encode_slice.h`](../../codec/encoder/core/inc/svc_encode_slice.h) — Slice encoding declarations
* [`codec/encoder/core/inc/svc_encode_mb.h`](../../codec/encoder/core/inc/svc_encode_mb.h) — Macroblock encoding declarations
* [`codec/encoder/core/inc/encode_mb_aux.h`](../../codec/encoder/core/inc/encode_mb_aux.h) — Forward DCT and quantization declarations
* [`codec/encoder/core/inc/deblocking.h`](../../codec/encoder/core/inc/deblocking.h) — Encoder in-loop deblocking filter declarations
* [`codec/encoder/core/inc/vlc_encoder.h`](../../codec/encoder/core/inc/vlc_encoder.h) — CAVLC entropy encoding declarations
* [`codec/encoder/core/inc/set_mb_syn_cabac.h`](../../codec/encoder/core/inc/set_mb_syn_cabac.h) — CABAC entropy encoding declarations
* [`codec/encoder/core/inc/nal_encap.h`](../../codec/encoder/core/inc/nal_encap.h) — NAL encapsulation & Annex B formatting
* [`codec/encoder/core/inc/ref_list_mgr_svc.h`](../../codec/encoder/core/inc/ref_list_mgr_svc.h) — SVC / LTR reference list manager declarations
* [`codec/encoder/core/inc/slice_multi_threading.h`](../../codec/encoder/core/inc/slice_multi_threading.h) — Slice multi-threading management
* [`codec/encoder/core/inc/wels_task_management.h`](../../codec/encoder/core/inc/wels_task_management.h) — Task manager and thread pool definitions

### Implementation Files (`.cpp`)
* [`codec/encoder/plus/src/welsEncoderExt.cpp`](../../codec/encoder/plus/src/welsEncoderExt.cpp) — Public C++ facade wrapper implementation
* [`codec/encoder/core/src/encoder.cpp`](../../codec/encoder/core/src/encoder.cpp) — Main encoder initialization and frame entry points
* [`codec/encoder/core/src/encoder_ext.cpp`](../../codec/encoder/core/src/encoder_ext.cpp) — Encoder extension methods
* [`codec/encoder/core/src/wels_preprocess.cpp`](../../codec/encoder/core/src/wels_preprocess.cpp) — Color space conversion and VAA analysis
* [`codec/encoder/core/src/ratectl.cpp`](../../codec/encoder/core/src/ratectl.cpp) — VGOP / Frame / GOM hierarchical rate control
* [`codec/encoder/core/src/svc_motion_estimate.cpp`](../../codec/encoder/core/src/svc_motion_estimate.cpp) — Diamond search & sub-pixel motion estimation
* [`codec/encoder/core/src/svc_mode_decision.cpp`](../../codec/encoder/core/src/svc_mode_decision.cpp) — Inter vs Intra RDO mode decision
* [`codec/encoder/core/src/md.cpp`](../../codec/encoder/core/src/md.cpp) — Mode decision helper functions
* [`codec/encoder/core/src/svc_encode_slice.cpp`](../../codec/encoder/core/src/svc_encode_slice.cpp) — Slice encoding loop
* [`codec/encoder/core/src/svc_encode_mb.cpp`](../../codec/encoder/core/src/svc_encode_mb.cpp) — Macroblock encoding loop
* [`codec/encoder/core/src/encode_mb_aux.cpp`](../../codec/encoder/core/src/encode_mb_aux.cpp) — 4x4 Forward integer DCT and quantization
* [`codec/encoder/core/src/deblocking.cpp`](../../codec/encoder/core/src/deblocking.cpp) — Encoder reconstruction deblocking filter
* [`codec/encoder/core/src/vlc_encoder.cpp`](../../codec/encoder/core/src/vlc_encoder.cpp) — CAVLC bitstream entropy encoding
* [`codec/encoder/core/src/svc_set_mb_syn_cavlc.cpp`](../../codec/encoder/core/src/svc_set_mb_syn_cavlc.cpp) — Macroblock CAVLC syntax serialization
* [`codec/encoder/core/src/set_mb_syn_cabac.cpp`](../../codec/encoder/core/src/set_mb_syn_cabac.cpp) — CABAC syntax serialization
* [`codec/encoder/core/src/svc_set_mb_syn_cabac.cpp`](../../codec/encoder/core/src/svc_set_mb_syn_cabac.cpp) — SVC CABAC syntax serialization
* [`codec/encoder/core/src/nal_encap.cpp`](../../codec/encoder/core/src/nal_encap.cpp) — Emulation prevention and Annex-B NAL packaging
* [`codec/encoder/core/src/ref_list_mgr_svc.cpp`](../../codec/encoder/core/src/ref_list_mgr_svc.cpp) — Temporal scalability and LTR reference management
* [`codec/encoder/core/src/slice_multi_threading.cpp`](../../codec/encoder/core/src/slice_multi_threading.cpp) — Multithreaded slice distribution
* [`codec/encoder/core/src/wels_task_management.cpp`](../../codec/encoder/core/src/wels_task_management.cpp) — Thread pool task scheduling

---

## 3. Common Subsystem (`codec/common/`)

### Header Files (`.h`)
* [`codec/common/inc/mc.h`](../../codec/common/inc/mc.h) — Motion compensation declarations
* [`codec/common/inc/sad_common.h`](../../codec/common/inc/sad_common.h) — SAD / SATD distortion cost calculation declarations
* [`codec/common/inc/deblocking_common.h`](../../codec/common/inc/deblocking_common.h) — Shared deblocking filter parameters and tables
* [`codec/common/inc/intra_pred_common.h`](../../codec/common/inc/intra_pred_common.h) — Shared intra prediction prototypes
* [`codec/common/inc/memory_align.h`](../../codec/common/inc/memory_align.h) — SIMD-aligned memory management helper
* [`codec/common/inc/WelsThreadPool.h`](../../codec/common/inc/WelsThreadPool.h) — Thread pool abstraction class

### Implementation Files (`.cpp`)
* [`codec/common/src/mc.cpp`](../../codec/common/src/mc.cpp) — C/C++ fallback motion compensation routines
* [`codec/common/src/sad_common.cpp`](../../codec/common/src/sad_common.cpp) — C/C++ fallback SAD/SATD cost calculation routines
* [`codec/common/src/memory_align.cpp`](../../codec/common/src/memory_align.cpp) — Aligned memory allocator implementation
* [`codec/common/src/WelsThreadPool.cpp`](../../codec/common/src/WelsThreadPool.cpp) — Thread pool worker thread implementation

---

## 4. Public API (`codec/api/`)

* [`codec/api/wels/codec_api.h`](../../codec/api/wels/codec_api.h) — Public C/C++ API interface definitions (`ISVCDecoder`, `ISVCEncoder`)
