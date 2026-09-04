# openh264-rs — a safe Rust rewrite of OpenH264

This directory holds a line-by-line Rust port of Cisco's [OpenH264](../README.md)
(reference version 2.6.0, the C++ in `../codec/`): the H.264/AVC encoder, the
decoder, and the encoder's video-processing plugins. It builds as a Rust library
and as a drop-in `libopenh264` shared library that exports exactly the seven
symbols upstream does, and it has no dependencies.

The one property everything here is organised around: **for the same input and
the same parameters, the port produces the same bytes as the C++**. Every
configuration axis the reference accepts is swept against the reference binary
and compared byte for byte, and that sweep is a gate, not a report.


## Building and using

```bash
cd rust/crates/openh264-rs
cargo build --release          # target/release/libopenh264_rs.{dylib,a} + the rlib
cargo test                     # debug profile builds at opt-level 3 with overflow checks on
```

As a Rust library:

```rust
use openh264_rs::*;

let mut enc = Encoder::new();
let mut param = SEncParamExt::default();
enc.default_params(&mut param);
param.iPicWidth = 320;
param.iPicHeight = 192;
param.sSpatialLayers[0].iVideoWidth = 320;
param.sSpatialLayers[0].iVideoHeight = 192;
assert_eq!(enc.initialize_ext(&param), 0);
let mut info = SFrameBSInfo::default();
// src: an SSourcePicture over I420 planes
// enc.encode_frame(&src, &mut info);

let mut dec = Decoder::new();
let dparam = SDecodingParam::default();
dec.initialize(&dparam);
// dec.decode(Some(&nal_unit), &mut planes, &mut buf_info);  one Annex-B NAL unit at a time
// dec.flush(&mut planes, &mut buf_info);                   drain at end of stream
```

Installing a trace callback (`set_trace_callback`) and the encoder's raw
`SetOption`/`GetOption` are `unsafe` on the Rust side: the callback and its
context are handed across the C ABI and must stay live and sound together for as
long as they are installed. Everything else on `Encoder` and `Decoder` is safe.

As a drop-in `libopenh264`: the cdylib exports `WelsCreateSVCEncoder`,
`WelsDestroySVCEncoder`, `WelsCreateDecoder`, `WelsDestroyDecoder`,
`WelsGetDecoderCapability`, `WelsGetCodecVersion` and `WelsGetCodecVersionEx` and
nothing else (`tools/abi_exports.sh` fails the build if the set changes). A C
consumer written against `codec_api.h` loads it as it would upstream's library;
`tools/abi_harness/` is that consumer, and it is a gate.

## The safety posture

The codec core is written in safe Rust over a small vocabulary in `src/safe/`:
plane cursors with stride arithmetic and negative offsets (`plane`), detachable bit
readers and writers (`bits`), pooled object graphs for the decoded-picture buffer
and reference lists (`pool`), and per-macroblock addressing (`mb_grid`). None of
these stores a borrow into a buffer; cursors are positions and buffers are
parameters, and every access lands in a slice index so an out-of-range read is a
panic rather than silent corruption. Where the C++ passes a pointer into the
middle of an array, the port passes the array and an index; where it aliases one
allocation through two pointers, the port owns it once and hands out views.

`unsafe` is confined to the C ABI in `src/api/` (the vtable thunks and the raw
option blobs `codec_api.h` defines), the intrinsics in `src/simd/`, one
`unsafe impl Sync` at the reconstruction seam, and test instruments.
Multi-threaded encoding forks with `std::thread::scope`; the context the workers
share is `Sync` by construction, which the compiler checks at the fork.

`src/simd/` is the one place `unsafe` is load-bearing rather than a boundary, so
the kernels are shaped to keep it small: each takes its bounds check on the safe
side and its pointer arithmetic on the unsafe side, never both in the same
expression. The macroblock copies are the clearest case — `RecCursor::block_span`
validates the whole block as one slice in safe code and states the invariant the
kernel relies on (`row y, column x` is at `y * stride + x`), and the `unsafe`
body does nothing but stride through what it was handed. The property is that a
wrong span becomes a panic and not a read past the plane, which is a claim a test
can hold: a mutant `block_span` one row's width short is caught by a block sized
to that gap.

**The compiler keeps it that way.** `#![forbid(unsafe_code)]` sits at the top of
every file that can carry it — 82 of 118 — so adding an `unsafe` block to one is a
build error, not a report someone has to read. The 12 files that cannot are the
C-ABI boundary, the intrinsics and the audited sites, and each `#[allow(unsafe_code)]`
in them carries its category and its reason at the site. `tools/find_dup_types.sh`
is a hand-run duplicate audit beside that.

