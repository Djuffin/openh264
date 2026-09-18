# openh264-rs

A zero-dependency, bit-exact Rust port of Cisco [OpenH264](../README.md) 2.6.0 (H.264/AVC encoder, decoder, and video-processing plugins), including specification fixes for B-slice inter prediction, cross-slice deblocking boundary strength, reference list modification, and Annex C DPB output reordering.

It builds as:
- A safe **Rust library** (`rlib`) exposing `Encoder` and `Decoder`.
- A drop-in **`libopenh264` shared/static library** (`cdylib` / `staticlib`) exporting the exact 7 symbols of `codec_api.h` (`WelsCreateSVCEncoder`, `WelsDestroySVCEncoder`, `WelsCreateDecoder`, `WelsDestroyDecoder`, `WelsGetDecoderCapability`, `WelsGetCodecVersion`, `WelsGetCodecVersionEx`), plus a `cxx` bridge.

## Build & Verify

```bash
cd rust/crates/openh264-rs
cargo build --release                  # Builds libopenh264_rs.{so,dylib,a,rlib}
cargo test                             # Runs unit, conformance, differential, and doc tests
bash ../../tools/abi_exports.sh        # Verifies cdylib exports exactly the 7 upstream C ABI symbols
bash ../../tools/cpp_interface_test.sh # Runs GTest C/C++ virtual-table & ABI interface tests
```

## Rust API Usage

```rust
use openh264_rs::*;

// Encode
let mut enc = Encoder::new();
let mut param = SEncParamExt::default();
enc.default_params(&mut param);
param.iPicWidth = 320;
param.iPicHeight = 192;
param.sSpatialLayers[0].iVideoWidth = 320;
param.sSpatialLayers[0].iVideoHeight = 192;
assert_eq!(enc.initialize_ext(&param), 0);

let mut bs_info = SFrameBSInfo::default();
// enc.encode_frame(&src_pic, &mut bs_info);

// Decode
let mut dec = Decoder::new();
dec.initialize(&SDecodingParam::default());
// dec.decode(Some(&nal_unit), &mut planes, &mut buf_info);
// dec.flush(&mut planes, &mut buf_info);
```

## Architecture & Safety

- **Safe Core (`src/encoder/`, `src/decoder/`, `src/common/`, `src/safe/`)**: Implemented in safe Rust over bounds-checked plane cursors (`plane`), bitstream readers/writers (`bits`), and pooled DPB/reference graphs (`pool`).
- **Modular API (`src/api/`)**:
  - `types.rs`: C-ABI compatible `#[repr(C)]` parameter/buffer structs and enums (`SBufferInfo`, `SEncParamExt`, etc.), layout-verified at compile time by `abi_guard.rs`.
  - `encoder.rs` & `decoder.rs`: Safe `Encoder` and `Decoder` wrappers (`#![deny(unsafe_code)]`). Only raw trace callbacks (`set_trace_callback`) and untyped `void*` option dispatch (`set_option_raw` / `get_option_raw`) are `unsafe`.
  - `c_api.rs` & `cxx_api.rs`: `ISVCEncoderVtbl` / `ISVCDecoderVtbl` thunks, `#[no_mangle]` C exports, and `cxx` bindings. Every FFI entry point is wrapped in `abi_guard!` (`std::panic::catch_unwind`) to prevent panics from crossing the ABI.
- **Confined `unsafe`**: Enforced via `#![forbid(unsafe_code)]` / `#![deny(unsafe_code)]` across the crate. `unsafe` is restricted to FFI pointer conversion boundaries (`src/api/`), target SIMD intrinsics (`src/simd/`), and scoped worker-thread lifetime erasure (`src/encoder/worker_pool.rs`).
