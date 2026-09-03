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

## Status

| | |
|---|---|
| Encoder | byte-identical with the C++ across 1043 harness configurations in both build profiles: every rate-control mode, both entropy coders, all four slice modes, 1 to 4 threads, all 52 QPs, long-term reference with feedback, all parameter-set strategies, 1 to 4 spatial layers with denoise, background detection, and screen content |
| Decoder | 58 conformance streams decode to the same frames as the reference; parity suites for parse-only decoding, no-delay decoding, error concealment, error reporting, resolution and reference-count changes, and a malformed-stream corpus refereed against the C++ decoder |
| Tests | about 600 unit and integration tests, plus compile-fail doctests pinning the crate's `unsafe` boundary |
| Upstream's own tests | Cisco's `test/api` gtest suite runs against the Rust shared library; one row is allowlisted, by design (see *Deliberate divergences*) |
| Unsafe code | 70 of 87 source files are `#![forbid(unsafe_code)]`; what remains is the C-ABI boundary in `src/api/` and a handful of audited sites, each tagged, counted, and ratcheted |
| Dependencies | none; `libc` only as a dev-dependency for the benches that load the C++ library |
| SIMD | none; the port is scalar throughout |

## Layout

```
rust/
├── crates/openh264-rs/          the crate (rlib + cdylib + staticlib, Rust 2024 edition)
│   ├── src/api/                 the C ABI: codec_api.h's types and vtables, the seven exports, option handling
│   ├── src/encoder/             codec/encoder — one module per C++ source file
│   ├── src/decoder/             codec/decoder — likewise
│   ├── src/processing/          codec/processing — the pre-processing plugins
│   ├── src/common/              codec/common — SAD/SATD, MC, intra prediction, deblocking, tracing
│   ├── src/safe/                the safe vocabulary the codec is written in (see below)
│   ├── tests/                   integration tests, incl. conformance and parity suites
│   ├── benches/                 c_vs_rust_bench (encoder, loads the C++ .dylib) and decode_1080p_bench
│   └── examples/portref.rs      the port's answer for one malformed-corpus entry
└── tools/
    ├── gates.sh                 the gate battery: commit | family | session | full | exit
    ├── diffharness/             the byte-parity harness: two drivers, compare.sh, sweep.sh, referees
    ├── abi_harness/             the shared library driven through dlopen as a consumer would; upstream's gtest suite against it
    ├── ecref/                   the C++ decoder's answer for the malformed corpus
    ├── unsafe_ratchet.sh, unsafe_census.sh, census.sh                      the unsafe and duplicate instruments
    ├── port_census.py, find_stub_bodies.py, find_dup_types.sh              the port-completeness audits
    └── perfpair.py              interleaved A/B benchmarking
```

Every Rust module keeps its C++ name, and every ported function keeps its C++ name
and cites the C++ `file:line` it came from, so the two trees stay diffable. The
`allow(non_snake_case, ...)` at the crate root is that policy, not debt.

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

## How correctness is checked

**The byte-parity harness** (`tools/diffharness/`). Two drivers with an identical
command line, `cxx_enc` linked against the reference's `libopenh264.a` and
`rust_enc` against this crate, encode the same YUV with the same parameters;
`compare.sh` runs both and `cmp`s the streams; `sweep.sh` runs presets over the
axes (`st mt qp def sl ltr ps dl bg scc`, 1043 rows in all). Before the harness
can run, the reference has to be built at the repository root:

```bash
make -j8 libraries binaries               # libopenh264.a, libopenh264.dylib, h264dec
bash rust/tools/diffharness/build.sh      # both drivers (RUST_ENC_PROFILE=debug|release)
bash rust/tools/diffharness/compare.sh res/CiscoVT2people_320x192_12fps.yuv 320 192 9 26 0 -1
bash rust/tools/diffharness/sweep.sh st   # one preset; `all` for every row
```

Two referees sit beside the bytes: `log_referee.sh` compares the two encoders'
trace output line by line, and `scc_verdicts.sh` compares the screen-content
preprocessor's scene-change decisions frame by frame under a configuration where
they must agree before the bytes can.

**The decoder** is refereed against the reference decoder (`h264dec`, and
`tools/ecref` for malformed input): conformance streams by per-frame hash, the
malformed corpus by return code, buffer status and frame hashes.

**The C-ABI boundary**: `tools/abi_exports.sh` (the export list), `tools/abi_sizes.sh`
(every ABI type's size and field offsets against the C headers, pinned in
`abi_guard.rs`), `tools/abi_harness/run.sh` (dlopen, conformance and encode
loopback through the .dylib, and Cisco's gtest suite against it, ratcheted against
`gtest_known_failures.txt`).

**The benches** double as referees: `c_vs_rust_bench` encodes every row through
both libraries in one process and prints the SHA-1 of each stream next to the
timing; a row that is not bit-identical is reported as such and fails the run.

**The gate battery** ties it together:

```bash
bash rust/tools/gates.sh commit    # build --all-targets, cargo test in both profiles, the unsafe ratchet   (~2 min)
bash rust/tools/gates.sh family    # + the differential sweeps in both profiles                            (~5 min)
bash rust/tools/gates.sh session   # + a Miri lane over the library
bash rust/tools/gates.sh exit      # + benches, Miri over the integration tests, the three C-ABI gates
```

Byte parity is the definition of done. A change that moves any sweep tally is
wrong by definition and is reverted, not explained.

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
option blobs `codec_api.h` defines), one `unsafe impl Sync` at the reconstruction
seam, and test instruments. Multi-threaded encoding forks with
`std::thread::scope`; the context the workers share is `Sync` by construction,
which the compiler checks at the fork.

Three instruments keep it that way: `tools/unsafe_ratchet.sh` counts raw pointers,
`unsafe fn`s and `unsafe` blocks per file against a committed baseline and fails on
any increase; `tools/unsafe_census.sh` pins the exact list of remaining
`#[allow(unsafe_code)]` sites by category; `tools/census.sh` fails on any duplicate
type, table or function declaration. A count that must move for a deliberate
change is regenerated in the same commit, with the reason.

## Deliberate divergences

Every place the port knowingly differs from the C++ is recorded at the site. The
ones a user can observe:

- **The decoder keeps the H.264 standard's POC tiebreak** where upstream's differs;
  one upstream gtest row (`DecoderOutputTest.CompareOutput/39`) is allowlisted for it.
- **Undefined behaviour becomes a defined refusal.** Where the C++ would write
  through a null pointer or index past an array on an input it cannot reach in
  practice, the port returns an error or clamps; each site says which C++ path it
  replaces and why the reachable outputs are unchanged.
- **An out-of-range `iRCMode` becomes `RC_QUALITY_MODE`**: a Rust enum cannot
  store the raw value the C++ keeps. Every value the reference accepts round-trips.

## Not ported, on purpose

- SIMD. Upstream's NEON, SSE and LSX kernels are not translated; the port is scalar
  everywhere. Byte parity holds because upstream's SIMD kernels are bit-exact with
  its C kernels on every path the harness runs; relative speed is a different
  question, and on Apple Silicon the reference (which dispatches NEON) is faster
  than the port on some rows.
- Upstream's threaded decoder. The decoder is single-threaded; every instance is
  independent and the multithreading test exercises instances on separate threads.
- `METHOD_IMAGE_ROTATE` and `METHOD_COLORSPACE_CONVERT`, two processing methods the
  encoder never requests (upstream implements only the first).
- `bEnableAdaptiveQuant` allocations: upstream forces the flag off in validation,
  so the buffers are never used; the plugin itself is ported.

## Known flake

Upstream's `SM_SIZELIMITED_SLICE` encoding with four threads is nondeterministic
in the reference (about one run in ten produces a different stream), which the
per-commit gate does not sample; those rows run in `sweep.sh all` under a retry
rule. The port's output on them is the reference's own majority output.

## Working on it

- Build the reference first; nothing here is verifiable without it.
- Add the harness knob before the feature: a configuration the drivers cannot
  express has no referee, and a port without a referee is a guess.
- Keep the C++ names and cite the C++ lines; port bodies whole, never stub them
  (`tools/find_stub_bodies.py` diffs each Rust body's call set against its C++
  original, and `tools/port_census.py --classify` lists every C++ function with no
  port and why).
- Run `gates.sh commit` before every commit and `gates.sh family` before anything
  that touches a coding path.

Licensed as upstream: BSD, Cisco Systems — see `../LICENSE`.
