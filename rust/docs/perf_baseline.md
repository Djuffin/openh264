# Performance baseline

The §7.4 budget anchor for the safety refactor. Recorded 2026-08-07 at `ac5b91d2`
(Phase 0, task T4), before any refactoring commit. Every later phase compares against
the medians here: ≤5% regression per phase, ≤10% cumulative at Phase 9.

## Machine

| | |
|---|---|
| CPU | Apple M1, 8 cores (4 performance + 4 efficiency) |
| Memory | 16 GB |
| OS | macOS 26.5 (25F71) |
| rustc | 1.97.1 (8bab26f4f 2026-07-14), stable-aarch64-apple-darwin |
| ffmpeg | 8.1.2, at `/opt/homebrew/bin/ffmpeg` |
| Profile | `cargo bench` (release, `[profile.bench]` defaults) |

Laptop-class hardware with heterogeneous cores and no thermal headroom guarantee.
Treat sub-5% differences between runs as noise; the per-row run values are printed
below so you can see each row's actual spread rather than trusting a single median.

## Two caveats that decide how these numbers may be used

### 1. Every C++-vs-Rust ratio is scalar-vs-scalar

`decode_1080p_bench` prints it directly:

```
C++ SIMD   : INACTIVE (WelsCPUFeatureDetect = 0x000000)
```

The C++ dylib links its NEON kernels but never dispatches them, and the Rust port has
no SIMD at all. So the ratios compare two scalar implementations. They are meaningful
as a regression detector and meaningless as a statement about libopenh264's real-world
speed.

### 2. Without ffmpeg the encoder bench measures the frame-skip path, not encoding

`c_vs_rust_bench` falls back to a synthetic pattern when it cannot run ffmpeg, and it
resolves ffmpeg from `PATH` only (unlike `decode_1080p_bench`, which finds
`/opt/homebrew/bin/ffmpeg` by itself). A non-interactive shell without Homebrew on
`PATH` therefore gets the fallback silently, apart from one warning line.

That fallback does not merely make the numbers pessimistic — it changes what is being
measured. High-entropy synthetic frames overshoot the default rate-control budget, so
from VGA upward nearly every frame is skipped, and the timing becomes the cost of
*deciding to skip*:

| | synthetic fallback | real lavfi content |
|---|---|---|
| 1080p Testsrc, 1 thread, Rust | 1.473 ms/frame | 4.279 ms/frame |
| 1080p vs QVGA ms/frame | 1.7× apart (27× the pixels) | 21× apart |
| `iContinualSkipFrames` warnings | 10 per 1080p row, 22 per VGA row | **zero** |
| C++ : Rust ratio at 1080p | 1.10× | 1.41× |

**So: always run the encoder bench with `FFMPEG` set.** The tables below are the real
content runs. A budget anchored on the fallback numbers would have been blind to any
regression in the encode kernels, because at VGA and above the kernels barely ran.

The measured fallback numbers are kept in the Phase 0 session log for the record; they
are not a baseline and must not be compared against.

## Reproducing

```bash
FFMPEG=/opt/homebrew/bin/ffmpeg BENCH_REQUIRE_FFMPEG=1 cargo bench --bench c_vs_rust_bench
```

```bash
cargo bench --bench decode_1080p_bench
```

Both from `rust/crates/openh264-rs/`. `BENCH_REQUIRE_FFMPEG=1` turns the silent
fallback into an error, which is what you want in a gate. Three runs each; the tables
report the median with all three values beside it.

## Encoder — `c_vs_rust_bench`

Median of 3 runs, ms/frame, lavfi content, `GetDefaultParams` + `InitializeExt`.
Every row was bit-identical between C++ and Rust in all three runs.

| configuration | frames | thr | C++ ms | Rust ms | ratio | Rust runs |
|---|---|---|---|---|---|---|
| 320x240 QVGA Moving Box | 200 | 1 | 0.265 | 0.200 | 1.32x | 0.198, 0.200, 0.200 |
| 320x240 QVGA Moving Box | 200 | 4 | 0.265 | 0.196 | 1.35x | 0.196, 0.196, 0.197 |
| 320x240 QVGA High-Contrast | 200 | 1 | 0.461 | 0.383 | 1.20x | 0.383, 0.380, 0.386 |
| 320x240 QVGA High-Contrast | 200 | 4 | 0.463 | 0.381 | 1.22x | 0.381, 0.380, 0.383 |
| 320x240 QVGA SMPTE Bars | 200 | 1 | 0.112 | 0.058 | 1.93x | 0.058, 0.056, 0.058 |
| 320x240 QVGA SMPTE Bars | 200 | 4 | 0.112 | 0.057 | 1.96x | 0.056, 0.057, 0.058 |
| 320x240 QVGA PAL 75% | 200 | 1 | 0.111 | 0.057 | 1.95x | 0.057, 0.057, 0.056 |
| 320x240 QVGA PAL 75% | 200 | 4 | 0.111 | 0.057 | 1.95x | 0.057, 0.057, 0.057 |
| 320x240 QVGA RGB Test | 200 | 1 | 0.121 | 0.066 | 1.83x | 0.066, 0.066, 0.066 |
| 320x240 QVGA RGB Test | 200 | 4 | 0.121 | 0.066 | 1.83x | 0.066, 0.065, 0.066 |
| 320x240 QVGA YUV Space | 200 | 1 | 0.112 | 0.058 | 1.93x | 0.058, 0.059, 0.057 |
| 320x240 QVGA YUV Space | 200 | 4 | 0.113 | 0.058 | 1.95x | 0.058, 0.059, 0.057 |
| 320x240 QVGA Spatial Ramps ⚠ | 200 | 1 | 0.218 | 0.153 | 1.42x | 0.124, 0.153, 0.220 |
| 320x240 QVGA Spatial Ramps ⚠ | 200 | 4 | 0.218 | 0.153 | 1.42x | 0.124, 0.153, 0.219 |
| 320x240 QVGA Mandelbrot | 200 | 1 | 0.999 | 0.885 | 1.13x | 0.882, 0.885, 0.887 |
| 320x240 QVGA Mandelbrot | 200 | 4 | 0.998 | 0.885 | 1.13x | 0.884, 0.885, 0.943 |
| 640x480 VGA Mandelbrot | 100 | 1 | 2.656 | 2.276 | 1.17x | 2.276, 2.259, 2.278 |
| 640x480 VGA Mandelbrot | 100 | 4 | 2.648 | 2.253 | 1.18x | 2.243, 2.253, 2.258 |
| 640x480 VGA SMPTE Bars | 100 | 1 | 0.430 | 0.215 | 2.00x | 0.215, 0.229, 0.214 |
| 640x480 VGA SMPTE Bars | 100 | 4 | 0.431 | 0.217 | 1.99x | 0.217, 0.216, 0.219 |
| 1280x720 720p Mandelbrot | 50 | 1 | 6.079 | 5.024 | 1.21x | 5.030, 5.020, 5.024 |
| 1280x720 720p Mandelbrot | 50 | 4 | 6.097 | 4.975 | 1.23x | 4.933, 4.975, 4.987 |
| 1280x720 720p SMPTE Bars | 50 | 1 | 1.703 | 1.042 | 1.63x | 1.042, 1.073, 1.032 |
| 1280x720 720p SMPTE Bars | 50 | 4 | 1.704 | 1.030 | 1.65x | 1.029, 1.030, 1.052 |
| 1920x1080 1080p Mandelbrot | 30 | 1 | 12.381 | 10.103 | 1.23x | 9.941, 10.103, 10.126 |
| 1920x1080 1080p Mandelbrot | 30 | 4 | 12.439 | 9.980 | 1.25x | 9.916, 9.980, 9.988 |
| 1920x1080 1080p SMPTE Bars | 30 | 1 | 4.491 | 2.902 | 1.55x | 2.865, 2.908, 2.902 |
| 1920x1080 1080p SMPTE Bars | 30 | 4 | 4.509 | 2.888 | 1.56x | 2.875, 2.914, 2.888 |
| 1920x1080 1080p Testsrc | 30 | 1 | 6.030 | 4.279 | 1.41x | 4.265, 4.279, 4.307 |
| 1920x1080 1080p Testsrc | 30 | 4 | 6.033 | 4.305 | 1.40x | 4.311, 4.305, 4.272 |

⚠ **Spatial Ramps is not a usable budget row.** Its three Rust runs span 0.124–0.220 ms
— a 77% spread, where every other row sits inside 3%. Do not read a regression into
that row without repeating it several times first; prefer 1080p Mandelbrot (the
heaviest row, ±1%) and QVGA Mandelbrot as the sensitive kernel indicators.

The 4-thread rows are within noise of the 1-thread rows because `iMultipleThreadIdc`
only splits *slices*, and these configurations code one slice per frame.

## Decoder — `decode_1080p_bench`

Median of 3 runs, ms/frame. 1920x1080, 60 frames, x264 @ 5 Mbit/s from lavfi
`testsrc2`. All three streams decoded 60/60 frames on both sides with matching
per-plane SHA-1 — the frame count is the load-bearing check here, because a decode
error drops frames silently and fewer frames reads as a speedup.

| stream | C++ ms | Rust ms | ratio | Rust runs | output SHA-1 |
|---|---|---|---|---|---|
| Constrained Baseline (CAVLC, no B-frames) | 2.342 | 2.190 | 1.07x | 2.182, 2.205, 2.190 | `0fba9a4e739dacd8` |
| Main (CABAC, B-frames) | 6.724 | 5.705 | 1.18x | 5.653, 5.705, 5.721 | `d8c07c43510fa4b4` |
| High (CABAC, B-frames, 8x8 transform) | 6.741 | 5.773 | 1.17x | 5.705, 5.790, 5.773 | `8b081cce0c478c9b` |

All three rows are stable to within 1.5% across runs, so this bench is the more
trustworthy of the two as a regression detector. The generated streams are cached
under `target/tmp/bench-streams/`; the SHA-1s above pin the decoded output, so a
changed hash means either the streams were regenerated differently or the decoder
changed.

## Phase 2 — leaf DSP kernels

Added as a column, never overwriting the Phase 0 anchor above. Each family records
its own paired measurement here as it lands.

**Session control, 2026-08-07 at `afcdd785`** (before any conversion), full battery
green: `cargo test` 375/0/20 and 373/0/20, sweeps 341/341 in both profiles,
`decode_1080p_bench` 60/60 with the three anchor SHA-1s unchanged.

The control's decode numbers came in ~2–3% above the Phase 0 anchor (2.257 / 5.834 /
5.876 against 2.190 / 5.705 / 5.773) with nothing but time between them. That is
toolchain and machine drift, and it is exactly why **the session control is the truth
and the anchor is only the long-run trend line**. Compare a family against the control
measured that same day, on the same tree, with the same binaries warm.

### T2 — `decoder/decode_mb_aux.rs` (the pilot)

Paired measurement, `decode_1080p_bench`, 3 runs per side, medians in ms/frame,
`ba13bdbd` (safe kernels present but unreachable — behaviourally the control) against
`ee7818fb` (the same kernels live behind shims). Same machine, quiescent, runs
interleaved by side.

| stream | control median | Phase 2 median | delta | control runs | Phase 2 runs |
|---|---|---|---|---|---|
| Constrained Baseline (CAVLC, no B) | 2.229 | 2.200 | **−1.3%** | 2.229, 2.232, 2.225 | 2.162, 2.201, 2.200 |
| Main (CABAC, B-frames) | 5.770 | 5.664 | **−1.8%** | 5.753, 5.779, 5.770 | 5.664, 5.664, 5.698 |
| High (CABAC, B, 8x8) | 5.820 | 5.744 | **−1.3%** | 5.808, 5.836, 5.820 | 5.732, 5.754, 5.744 |

Negative is faster. Every run on both sides decoded 60/60 frames with the three anchor
SHA-1s, so this is a like-for-like timing comparison and not a shorter workload.

**The conversion is a small win, not a cost**, and the per-side spreads (≤1%) are
tight enough for that to mean something. The plausible mechanism is the one deliberate
restructuring: the 4x4 IDCT's write loop was transposed from column-major to row-major,
turning four strided single-byte stores per column into four contiguous stores per row
through a `&mut [u8; 4]` window. Bounds checks did not show up at all — one per row,
against sixteen samples of arithmetic.

Encoder side: unmeasured for this family, correctly — `decode_mb_aux.rs` is decoder-only.
`c_vs_rust_bench` was run anyway as a correctness gate (all rows bit-identical) and the
release sweep held its ~21s wall time.

### T3 — `decoder/get_intra_predictor.rs` (the designated worst case)

Same protocol: `b7f48311` (42 safe kernels present but unreachable) against `a4828187`
(the same kernels live behind shims), 3 runs per side, medians in ms/frame.

| stream | control median | Phase 2 median | delta | control runs | Phase 2 runs |
|---|---|---|---|---|---|
| Constrained Baseline (CAVLC, no B) | 2.243 | 2.247 | +0.2% | 2.252, 2.243, 2.243 | 2.211, 2.247, 2.249 |
| Main (CABAC, B-frames) | 5.810 | 5.762 | −0.8% | 5.768, 5.826, 5.810 | 5.761, 5.786, 5.762 |
| High (CABAC, B, 8x8 transform) | 5.814 | 5.821 | +0.1% | 5.825, 5.811, 5.814 | 5.821, 5.808, 5.856 |

60/60 frames and the three anchor SHA-1s on every run, both sides.

**The phase's designated worst case is a wash** — every row inside ±1%, which is the
per-side spread. This is the family plan §9's risk register names ("perf regression in
MD/ME/intra hot loops … convert worst case early"), and it is 42 kernels with the
highest pointer-arithmetic density in the tree, ~140 punned accesses, converted to
bounds-checked slice access. The risk row can be downgraded.

Why it costs nothing is worth stating, because it generalises: **the bounds checks land
per row, not per sample.** A 4x4 mode does one `row_mut` per output row (4 checks for 16
samples) and reads its neighbours into fixed-size arrays with one check each; the 8x8
modes do ~50 arithmetic operations per row against one check. The old code's advantage —
unaligned `u32`/`u64` stores covering 4 or 8 samples at once — survives too, because
`copy_from_slice` and `fill` on a fixed-size window compile to the same wide stores.

**One caveat, stated rather than smoothed over.** The session-end battery's own bench run
— taken minutes after a full `cargo test` in both profiles, on a machine that had been
building all session — read 2.251 / 5.830 / 5.873, i.e. back at the control's level
rather than the paired medians'. Nothing regressed between them; the difference is
between-invocation machine state, and it is the same order as the effect being measured.
**Trust the paired table, not a single run from a battery**: it is the only measurement
here where both sides were taken back-to-back under identical conditions. The practical
rule for later families is the one this makes obvious — a budget claim needs its own
paired run, and `gates.sh`'s bench line is a correctness gate (frame counts and hashes),
not a budget one.

## How to use this in later phases

1. **Compare medians, not single runs**, and check the per-row spread column first.
   A row whose baseline spread is 77% cannot detect a 5% regression.
2. **The decoder bench is the sharper instrument.** Phase 2 (kernels) and Phase 5
   (decoder pivot) should treat its three rows as the primary budget.
3. **1080p Mandelbrot** is the encoder's heaviest and most stable row — use it as the
   primary encoder budget, with QVGA Mandelbrot as the small-frame counterpart.
4. **Always `BENCH_REQUIRE_FFMPEG=1`.** A silent fallback would turn a real regression
   into a flat line.
5. Expect *wins* in Phase 4 (direct calls replacing indirect dispatch) and losses in
   Phase 2 (bounds checks in 4×4 loops). Both are budgeted per phase, not per commit.
