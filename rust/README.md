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
| Tests | 657 unit and integration tests in release, 664 in debug, plus compile-fail doctests pinning the crate's `unsafe` boundary |
| Upstream's own tests | Cisco's `test/api` gtest suite runs against the Rust shared library; one row is allowlisted, by design (see *Deliberate divergences*) |
| Unsafe code | 81 of 128 source files are `#![forbid(unsafe_code)]`, which the compiler checks on every build; what remains is the C-ABI boundary in `src/api/`, the intrinsics in `src/simd/x86_64/` and `src/simd/aarch64/`, and a handful of audited sites, each tagged with `#[allow(unsafe_code)]` and a reason |
| Dependencies | none; `libc` only as a dev-dependency for the benches that load the C++ library |
| SIMD | Three kernel sets behind one alias, each a parity port of the scalar beside it, plus the scalar forwards. `src/simd/x86_64/` is the default on x86_64: 85 SSE2 kernels plus an AVX2 pair for 16-wide SAD, across SAD/SATD, motion compensation, forward and inverse DCT, quantization, intra prediction, deblocking, macroblock copies and CAVLC scoring — **1.43x** on the encoder there. `src/simd/aarch64/` is the default on aarch64: the same entry points as NEON intrinsics, transcribed from upstream's own arm64 assembly — **2.50x** per kernel and **1.45x** on the encoder on an Apple M1, bit-identical with the C++ on every row (see *Performance*). `--features wide` swaps in `src/simd/wide/`, the same entry points written in the `wide` crate's portable lane types, which dispatches on **every** target. No MMX or LSX |

## Layout

```
rust/
├── crates/openh264-rs/          the crate (rlib + cdylib + staticlib, Rust 2024 edition)
│   ├── src/api/                 the C ABI: codec_api.h's types and vtables, the seven exports, option handling
│   ├── src/encoder/             codec/encoder — one module per C++ source file
│   ├── src/decoder/             codec/decoder — likewise
│   ├── src/processing/          codec/processing — the pre-processing plugins
│   ├── src/common/              codec/common — SAD/SATD, MC, intra prediction, deblocking, tracing
│   ├── src/simd/                CPU feature detection and four kernel sets: x86_64/ (SSE2 intrinsics),
│   │                            aarch64/ (NEON intrinsics, ported from upstream's arm64 asm), wide/ (the `wide`
│   │                            crate, safe, behind `--features wide`) and scalar/ (forwards, so a target with
│   │                            none of the others still resolves); simd::kernels aliases the selected one
│   ├── src/safe/                the safe vocabulary the codec is written in (see below)
│   ├── tests/                   integration tests, incl. conformance and parity suites
│   ├── benches/                 c_vs_rust_bench and decode_1080p_bench (both load the C++ .dylib),
│   │                            and kernel_bench (per kernel: scalar, the host's intrinsics, wide), Rust only
│   └── examples/portref.rs      the port's answer for one malformed-corpus entry
└── tools/
    ├── gates.sh                 the gate battery: commit | family | session | full | exit
    ├── diffharness/             the byte-parity harness: two drivers, compare.sh, sweep.sh, referees
    ├── abi_harness/             the shared library driven through dlopen as a consumer would; upstream's gtest suite against it
    ├── ecref/                   the C++ decoder's answer for the malformed corpus
    ├── find_stub_bodies.py, find_dup_types.sh                              the port-completeness audits
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

**The SIMD kernels** are held to the same standard as the port itself, one level
down: each is a parity port of the scalar beside it, checked against that scalar
directly by a unit test — exhaustively where the input space allows it, and over
the full `i16` range where it does not, because a kernel that agrees on small
coefficients and diverges at the extremes would pass a sampled test and corrupt a
real stream. **Which kernel set a build has is a compile-time question**, exactly as
it is upstream — `codec/` contains no environment switch, and a scalar reference is
`USE_ASM=No` at the Makefile. `--features scalar` is that flag here: it points
`simd::kernels` at the scalar forwards and clears the feature word, so the `pfXxx`
tables install their scalar arm too. One flag, the whole tier, and the suite runs
each way:

```bash
cargo test --release                      # on whatever kernels this target has: SSE2 on x86_64, NEON on aarch64
cargo test --release --features scalar    # the same tests, same bytes, no kernels at all
cargo test --release --features wide      # the same again, on the portable kernels
```

A kernel that is written but never reached is the failure this arrangement is
most exposed to — a slot can be left pointing at the scalar and nothing observable
changes, because the output of a parity port *is* the scalar's output — so
reachability is asserted, not assumed. `every_kernel_is_named_by_a_dispatch_site`
reads the source and requires every entry point of `simd::x86_64`, `simd::aarch64`
and `simd::wide` to be named through the `simd::kernels` alias somewhere outside
`src/simd/`, or to be on a short list of module-internal workers. That list is the
only hand-kept part and it fails closed: a new kernel nothing dispatches is not on
it. The bug is not hypothetical — it has happened twice here, to two Hadamard
"kernels" that were the scalar copied verbatim and to the `BLOCK_4x4`/`8x4`/`4x8`
SAD slots, whose kernels were written and tested with no table entry.
`the_kernel_sets_expose_the_same_entry_points` is the other half: the four sets
must agree, or a dispatch site would compile on one target and not another.

**The benches** double as referees: `c_vs_rust_bench` encodes every row through
both libraries in one process and prints the SHA-1 of each stream next to the
timing; a row that is not bit-identical is reported as such and fails the run.
Running it once per feature is how one kernel set is measured against another, and
the SHA-1 column is what says the sets agree.

**The gate battery** ties it together:

```bash
bash rust/tools/gates.sh commit    # build --all-targets, cargo test in both profiles                       (~2 min)
bash rust/tools/gates.sh family    # + the differential sweeps in both profiles                            (~5 min)
bash rust/tools/gates.sh session   # + a Miri lane over the library
bash rust/tools/gates.sh exit      # + benches, Miri over the integration tests, the three C-ABI gates
```

Byte parity is the definition of done. A change that moves any sweep tally is
wrong by definition and is reverted, not explained.

## Performance

A kernel set is chosen when the binary is built, so measuring one against another is
building twice. `kernel_bench` is the per-kernel instrument and needs neither ffmpeg
nor a reference build; `c_vs_rust_bench` is the whole-encoder one and needs both.

```bash
cd rust/crates/openh264-rs
cargo bench --bench kernel_bench --features wide     # scalar, the host's intrinsics and wide, one table
cargo bench --bench c_vs_rust_bench                  # this target's kernels, against the C++
cargo bench --bench c_vs_rust_bench --features scalar # the same rows with no kernels at all
```

Single-threaded, one slice, best of two alternating passes, on an x86_64 host
reporting SSE2 through AVX2 — the default build against the same build with
`--features scalar`:

| clip | scalar fps | SIMD fps | speedup |
|---|---:|---:|---:|
| 320x240 QVGA high-contrast | 1271 | 2041 | 1.61x |
| 320x240 QVGA mandelbrot | 587 | 966 | 1.65x |
| 640x480 VGA SMPTE bars | 1912 | 2165 | 1.13x |
| 640x480 VGA mandelbrot | 235 | 380 | 1.62x |
| 1280x720 720p SMPTE bars | 498 | 564 | 1.13x |
| 1280x720 720p mandelbrot | 119 | 183 | 1.55x |
| | | geometric mean | **1.43x** |

All six bitstreams hash identically across the two builds. That is checked, not
assumed: every bench row prints its stream's SHA-1 beside the timing, and a set of
kernels that changed the output would show up there rather than as a speedup.

**The spread is the result, not noise.** SMPTE bars is flat colour fields, so most
macroblocks resolve to skip or static and the time goes to mode decision and
entropy coding, which have no kernels. Mandelbrot forces full residual work on
every block, which is where the kernels are. 13% to 65% is the honest range for
"how much does the SIMD tier buy", and which end a real clip lands on depends on
its content, not on the port.

Two things the table does not say. It is single-threaded on purpose, to keep
scheduling out of the measurement — more threads give a smaller ratio, because
threading recovers some of the same wall clock. And it is the encoder;
the shared kernels in `src/common/` serve the decoder too, but the decode path
has not been measured this way.

### aarch64: the NEON set

`src/simd/aarch64/` is the default there, and it is the same table measured the
same way — single-threaded, one slice, best of two alternating passes, the default
build against `--features scalar` — on an Apple M1:

| clip | scalar fps | NEON fps | speedup |
|---|---:|---:|---:|
| 320x240 QVGA high-contrast | 1696 | 2656 | 1.57x |
| 320x240 QVGA mandelbrot | 803 | 1376 | 1.71x |
| 640x480 VGA SMPTE bars | 2076 | 2169 | 1.04x |
| 640x480 VGA mandelbrot | 322 | 554 | 1.72x |
| 1280x720 720p SMPTE bars | 579 | 668 | 1.15x |
| 1280x720 720p mandelbrot | 154 | 262 | 1.70x |
| | | geometric mean | **1.45x** |

Over all fifteen rows of `c_vs_rust_bench` the geometric mean is 1.30x, with the
same spread and for the same reason: flat colour fields spend their time where there
are no kernels. Every row's bitstream hashes identically to the C++'s, which on this
target is the reference's own NEON assembly — so the table is also the check that
the transcription is exact. Per kernel, `kernel_bench` puts the NEON set at
**2.50x** the scalar (geometric mean over 41 kernels), from 8.1x on the chroma MC
and 6-7x on the DC IDCT, the intra plane predictors and the horizontal deblocking
filters, down to parity on the 16x16 copy and the dequantiser, where LLVM's
auto-vectorised scalar is already the same instructions.

The kernels are transcriptions of `codec/*/arm64/*_aarch64_neon.S`, and each file's
header says where it departs from its asm. There are four such places, and every
one is a 16-bit lane in the asm that wraps where the C's `int` does not — the
quantiser's biased magnitude, the two IDCTs' passes, the centre MC filter's
horizontal pass — widened here so the byte-parity contract holds over the whole
input range; the unit tests drive each one to the end of that range. Against the
C++ itself the port encodes at 0.55x on this target, which is the same picture as
on x86_64: the kernels are now equal, and what remains is the scaffolding around them.

### The portable tier

`--features wide` points every dispatch site at `src/simd/wide/` instead of the
host's intrinsic set. Those kernels are `wide`-crate lane types throughout — no
`core::arch`, every file `#![forbid(unsafe_code)]` — so unlike the intrinsics they
are reachable on any target, and on aarch64 LLVM lowers their lanes to NEON. On
x86_64 they keep about 88% of what the intrinsics buy — 1.35x on the encoder
against the intrinsics' 1.42x, the gap being the SAD family, where the portable API
has no `psadbw` and pays seven operations a row instead of two.

On the M1 they are **1.34x** per kernel against the scalar and **0.54x** against
the NEON set, and before the NEON set existed they measured **0.93x** on the encoder
end to end. The kernels that lose are the ones the encoder spends its time in: the
half-pel MC filters run at 0.27x-0.60x of the scalar and the SATDs at 0.58x-0.73x,
against wins of 2.7x-5.1x on the intra plane predictors, the horizontal deblocking
filters, the quantisers and the i16x16 DC IDCT — which are called far less often.
The cause is that `simd/wide/` is shaped around what SSE2 *lacks*: it emulates
`pavgb` as `(a | b) - ((a ^ b) >> 1)` and spells permutes as array casts, because the
SSE2 baseline has neither. NEON has `urhadd` and `tbl`, and the hand-written set uses
them — so on aarch64 those emulations are overhead paid against kernels that never
needed them. **A `wide` port that wins here would have to be written to what NEON
has, not to what SSE2 is missing.**

The bytes hold either way: all `c_vs_rust_bench` rows are identical with the wide
kernels dispatched, and the benches fail if they are not.

```bash
cargo bench --bench kernel_bench --features wide       # scalar, the host's intrinsics and wide, one table
```

Against the C++ reference rather than against itself, use `c_vs_rust_bench`, which
needs the reference library built first — once per feature for one kernel set
against another, since a build has exactly one.

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

- **Upstream's arm64 kernels outside the ten families the port has slots for.**
  `src/simd/aarch64/` covers every entry point `src/simd/x86_64/` has, and upstream's
  arm64 tree has kernels the port's tables have no slot for: the decoder's
  directional I4x4 predictors and the `DC_TOP`/`DC_LEFT` variants (scalar here on
  every target, as on x86_64), the `Intra*Combined3` SAD/SATD searches (whose slots
  the port deleted), the boundary-strength calculator, `ExpandPicture`, and the VAA,
  down-sampler and adaptive-quantisation families. 32-bit ARM NEON and LoongArch
  LSX are not translated at all.
- **MMX.** Upstream keeps four kernels in MMX only — the 8-wide macroblock copies
  and the decoder's i4x4 directional predictors. MMX aliases the x87 register file,
  so every kernel has to be paired with an `emms` and the whole tier has to agree
  about it; the copies are instead written with `movq` in its SSE2 encoding, which
  is the same move without the state.
- **Two SSE2 kernels that lose to the compiler**, both written, checked against the
  scalar, measured, and then dropped rather than shipped. `WelsScan4x4DcAc_sse2`'s
  transcription is 21 instructions against the scalar's 13, because `movdqu` lets
  LLVM issue the second load at a 7-word offset and skip the `pextrw`/`pinsrw` chain
  the asm needs after its `movdqa`. `WelsNonZeroCount_sse2`'s `pminub` and the
  scalar's `pcmpeqb`/`pandn` are the same ten instructions, and LLVM folded the
  dispatch branch away on the grounds that both arms were the same code. Each
  decision is recorded at the scalar it applies to, with the numbers and the
  command to re-check them.
- **The SSSE3, SSE4.x and wider AVX2 tiers**, and the VAA, feature-search,
  downsampler and denoise kernel families. `src/simd/x86_64/` is SSE2 plus one AVX2
  pair; the rest of upstream's x86 tree is still scalar here.
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
  original).
- Run `gates.sh commit` before every commit and `gates.sh family` before anything
  that touches a coding path.

Licensed as upstream: BSD, Cisco Systems — see `../LICENSE`.
