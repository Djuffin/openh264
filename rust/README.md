# openh264-rs — a safe Rust rewrite of OpenH264

This directory holds a line-by-line Rust port of Cisco's [OpenH264](../README.md)
(reference version 2.6.0, the C++ in `../codec/`): the H.264/AVC encoder, the
decoder, and the encoder's video-processing plugins. It builds as a Rust library
and as a drop-in `libopenh264` shared library that exports exactly the seven
symbols upstream does, and it has no dependencies. The reference here is 2.6.0
plus a small set of local decoder patches; upstream is still affected by each,
this tree is not, and the port matches the patched tree:

* `rec_mb.cpp`'s `GetInterBPred` combined a B partition's two hypotheses into one
  destination, so bi-predicted 16x8, 8x16 and 4x4 sub-partitions came out wrong.
* `deblocking.cpp` gave a B_Skip macroblock the P_Skip short-cut (internal edge
  bS = 0) although its four 8x8 quadrants carry per-8x8 or per-4x4 direct motion.
* `parse_mb_syn_cabac.cpp` and `parse_mb_syn_cavlc.cpp` wrote a temporal direct
  sub-macroblock's reference indices into the MV-prediction cache before the
  non-direct sub-macroblocks were predicted (CABAC), or as -1 (CAVLC), defeating
  the "not yet decoded partition is unavailable" rule of 6.4.11.7 / 8.4.1.3.2.
* `mv_pred.cpp`'s `GetColocatedMb` did not recognise a co-located P_8x8ref0 as
  8x8-partitioned, so with `direct_8x8_inference_flag = 0` the direct derivation
  ran at 8x8 instead of 4x4 granularity.
* `manage_dec_ref.cpp`'s two reference-list modification routines processed at
  most `num_ref_frames + 2` commands and padded the rest, instead of running one
  per entry of a list `num_ref_idx_lX_active` long (8.2.4.3).
* `rec_mb.cpp`'s `GetInterBPred` advanced the destination before applying explicit
  weights, so only the second half of a uni-predicted 8x4 or 4x8 B sub-partition was
  weighted, where 8.4.2.3 weights every partition.
* `welsDecoderExt.cpp` put pictures out by a heuristic — buffer nothing until a B
  slice has been seen, then emit whatever is within two of the last written POC —
  instead of the bumping process of Annex C, so a conforming stream's pictures could
  come out permuted.

### Picture output order

The last of those is the one that changes what a caller sees, so it is worth
spelling out. Upstream's display layer holds a decoded picture back only once a B
slice has appeared (`bHasBSlice`), and then emits the smallest buffered POC when it
is within one of the last written POC, or when the decoder has already moved past
it; otherwise it emits in decoding order. On a stream whose anchors are coded
before the B pictures they bracket, that decides to emit a P picture before the B
pictures that precede it in output order have been seen, and the pictures come out
in the wrong order — measurably, against the JVT gold, on `CVBS3_Sony_C`,
`CACQP3_Sony_D`, `CVWP2_TOSHIBA_E`, `CVWP3_TOSHIBA_E` and `CABAST3_Sony_E`.

This tree runs the specification's process instead. Pictures are emitted in
(coded video sequence, POC) order — sequences in decoding order, POC ascending
within each, a new sequence at every IDR, SPS change and `memory_management_control_operation`
equal to 5 — and one is emitted per completed picture as soon as C.4.5.3 says the
DPB has no empty frame buffer, where the DPB size is A.3.1's over Table A-1
(`MaxDpbMbs / PicSizeInMbs`, capped at 16, overridden by the VUI's
`max_dec_frame_buffering`, never below `max_num_ref_frames`). A stream whose VUI
carries `max_num_reorder_frames` also emits as soon as more than that many pictures
are waiting, which is E.2.1's guarantee and keeps such a stream at its encoder's
latency rather than its level's.

Three families skip the buffer entirely and are handed each picture the moment it
is decoded, as the layer always did for baseline: profile 66 and 83; any
`pic_order_cnt_type` other than 0 (type 2 has output order equal to decoding order
by definition, and this decoder derives no POC for type 1); and a VUI that says
`max_num_reorder_frames` is 0 — which is what this project's own encoder writes, so
openh264-encoded streams keep zero latency.

The price is latency, and it is the JM's and `ffmpeg -strict strict`'s price too: a
Main or High stream with `pic_order_cnt_type` 0 and no VUI now has its pictures held
for up to `dpb_size - references_held` completed pictures, even when it turns out to
contain no B slices at all, because nothing in such a stream says a B picture is not
coming. Ten of this tree's assets are in that position — same order, same bytes,
delivered later. The decoded picture pool grows to `dpb_size + max_num_ref_frames + 3`
for streams that reorder, and is unchanged for the rest.

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
`unsafe impl Sync` at the reconstruction seam, one lifetime erasure in the
encoder's worker pool (`src/encoder/worker_pool.rs`), and test instruments.
Multi-threaded encoding forks onto a pool of persistent worker threads through a
`scope` API shaped like `std::thread::scope`, so the context the workers share is
`Sync` by construction, which the compiler checks at the fork. The pool's one
`unsafe` erases a job closure's `'scope` lifetime to hand it to a long-lived
thread; it is sound for the reason `std::thread::scope` is — the scope waits for
every job to complete, on its return and on its unwind path, before the frame the
job borrows can end — and the argument is written at the top of that module. The
pool is built with the encoder's slice-threading resources when
`iMultipleThreadIdc > 1`, is never constructed for single-threaded encoding, and
its threads are joined when the encoder is uninitialised.

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
every file that can carry it — 82 of 133 — so adding an `unsafe` block to one is a
build error, not a report someone has to read. The files that cannot are the
C-ABI boundary, the intrinsics, the audited sites and the worker pool, and each
`#[allow(unsafe_code)]` in them carries its category and its reason at the site. `tools/find_dup_types.sh`
is a hand-run duplicate audit beside that.

