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

### T4 — `common/mc.rs` (motion compensation) — **over budget, and the first family that is**

Protocol changed here, deliberately. T3's caveat — "trust the paired table, not a
single run from a battery" — turned out to understate the problem: sequential
`cargo bench` invocations of the *same binary* drifted ~3% on the Rust rows while the
C++ rows in the same runs held to ±0.3%. So this family was measured by building both
binaries, keeping them on disk, and **interleaving control and candidate runs inside
one loop**. Every number below comes from that, 3 pairs, medians in ms/frame.

| stream | control median | Phase 2 median | delta | control runs | Phase 2 runs |
|---|---|---|---|---|---|
| Constrained Baseline (CAVLC, no B) | 2.232 | 2.415 | **+8.2%** | 2.232, 2.228, 2.248 | 2.413, 2.421, 2.415 |
| Main (CABAC, B-frames) | 5.716 | 6.129 | **+7.2%** | 5.716, 5.712, 5.737 | 6.157, 6.129, 6.098 |
| High (CABAC, B, 8x8) | 5.770 | 6.176 | **+7.0%** | 5.770, 5.768, 5.773 | 6.176, 6.194, 6.163 |

60/60 frames and the three anchor SHA-1s on every run, both sides. **This exceeds the
§7.4 per-family budget of 5%.**

#### What the investigation found, in the order it found it

The first measurement was **+33% / +20% / +20%**. Three defects, each found by
profiling rather than by reading, account for the difference between that and the
number above.

1. **`copy_from_slice` on a runtime length is a `memmove` call.** `_platform_memmove`
   went 12 → 55 profile samples. The C++ copies a fixed 16/8/4/2 bytes per row through
   `LD64`/`ST64A8`; a generic `copy_from_slice(width)` cannot see the width and calls
   out. Making the width a **const generic** (`copy_rows::<16>`) and dispatching
   `mc_copy` to the four instantiations — which is exactly what the C++ dispatch
   does — took `McLuma_c(0, 0)` on a 16x16 block from **10.8x to ~1x**. This is the
   single biggest item, and the zero-MV copy is the commonest phase in real content.
2. **Indexing several same-length slices by `j` is not free.** The vertical 6-tap
   reads six rows; indexing all six plus the output by `j` leaves LLVM seven bounds
   facts to prove per *sample* and it does not prove them. Zipping the seven
   iterators moves the check to one per row. Combined with a **rolling row window**
   (rotate five slices down, fetch only the new bottom row, so one
   `center + dy * stride` multiply per row instead of six), the luma micro-benchmark
   total went 1.22x → 1.09x.
3. **Attribute parity matters.** Every C++ kernel here is `inline` and the twelve
   quarter-pel composites are reached through a *table*, so each is out of line with
   the inner kernels inlined into it. Reproducing that — `#[inline(always)]` on the
   inner kernels, `#[inline(never)]` on the composites — is worth a few percent;
   letting the sixteen-way `match` inline everything into one frame is not.

#### What remains, and why it is not another bug

The residual ~7% is **fixed per-call overhead**, not per-sample cost. The evidence:

* A 16x16 luma block through the shim runs at 0.85–1.30x the old code depending on
  phase, and the centre kernels are now *faster* than the raw ones (0.88–0.99x).
* An 8x8 chroma **copy** runs at ~2.9x — 3.5 ns to 10.4 ns. The kernel is trivial;
  the whole difference is the shim's span arithmetic (three multiplies), two
  `from_raw_parts`, four constructor asserts, and the per-row `idx` multiply and
  bounds check that replace the C++'s pointer increment.
* That overhead is ~7–16 ns per call, and MC is called *very often with small blocks*
  — every inter partition, plus two chroma calls for each. At roughly 24k luma and
  48k chroma calls per 1080p frame, 7 ns of chroma overhead alone is ~0.34 ms against
  a 2.2 ms frame.

Two structural hypotheses were tested and **rejected by measurement**, so nobody
repeats them: passing the cursors by value instead of by reference (a wash: 2.372 vs
2.344), and making `McLuma_c` dispatch over the raw per-phase shims so every span
computation folds to constants (a wash: +8.3/+6.1/+5.9 against +8.2/+7.2/+7.0, and it
would have left two dispatch implementations to keep in agreement).

#### The row walker, built and rejected

The obvious systematic fix was tried, at Eugene's direction, and it **loses**. The
hypothesis was that the residual is per-row cost, so `PlaneCursor::rows` /
`PlaneCursorMut::rows_mut` were added to the Phase 1 API — one bounds check for the
whole block, then `chunks(stride)` walking by pointer addition instead of
`row()`/`row_mut()` recomputing `center + dy * stride` and re-checking per row — and
every kernel here was re-fitted onto them.

| variant | Constrained Baseline | Main | High |
|---|---|---|---|
| `row()` per row (what landed) | +8.2% | +7.2% | +7.0% |
| `rows()` everywhere | **+23.6%** | **+13.5%** | **+13.2%** |
| `rows()` in the copy path only | **+14.4%** | **+10.2%** | **+10.2%** |

Both directions measured against the same control binary, interleaved. The second row
could be blamed on the seven-deep `Zip` the 6-tap kernels need; the third cannot — it
is `copy_rows`, a two-way zip, the simplest loop in the file, and it is still 6 points
worse. So the conclusion is about the iterator and not about the nesting:
`Chunks::next` is a `min` plus a `split_at` plus a runtime-length slice that then needs
`[..WIDTH]` and a `try_into`, where `row()` gives a statically-sized window that folds.
A multiply per row is cheaper than that.

`rows`/`rows_mut` were therefore **removed from `safe/plane.rs`** rather than left in
place: an unused API that measures worse is a trap for T5–T8, which would reach for it
on the strength of its doc comment. Anyone tempted to re-add it should read this table
first.

That leaves the residual ~7% as per-call overhead in the shim layer — span arithmetic,
two `from_raw_parts`, four constructor asserts — which is **scaffolding Phase 5
deletes**, not a property of the safe kernels. The kernels themselves are at parity or
better: the centre filters measure 0.88–0.99x against the raw ones.

### T5 — `common/sad_common.rs` + `common/intra_pred_common.rs` — **swap reverted for SAD**

The first family whose swap was measured and **taken back out**. Also the first time
the microbenchmark and the end-to-end bench disagreed, and the microbenchmark was
wrong. Both facts matter more than the numbers.

**The encoder measurement that decided it.** `c_vs_rust_bench` with `FFMPEG` set,
control (`82014c9d`, before any T5 code) against commit B (`209d3c66`), binaries kept
on disk and **interleaved**, 3 pairs, medians in ms/frame. Rust side only.

| stream | thr | control | commit B | delta |
|---|---|---|---|---|
| 320x240 Mandelbrot | 1 | 0.902 | 0.927 | +2.77% |
| 640x480 Mandelbrot | 1 | 2.288 | 2.380 | +4.02% |
| 720p Mandelbrot | 1 | 4.969 | 5.210 | +4.85% |
| 1080p Mandelbrot | 1 | 9.938 | 10.554 | +6.20% |
| 1080p Testsrc | 1 | 4.271 | 4.799 | +12.36% |
| 1080p SMPTE Bars | 1 | 2.914 | 3.400 | +16.68% |
| 720p SMPTE Bars | 1 | 1.085 | 1.279 | +17.88% |
| 640x480 SMPTE Bars | 1 | 0.240 | 0.289 | +20.42% |

Median across all 30 stream/thread rows **+13.85%**, worst **+20.42%**. §7.4's hard
ceiling is 10% on any stream at any commit, so this is a breach, and a breach stops
the phase for recovery.

**Bisected by file**, same protocol, 2 pairs each. The regression is entirely SAD:

| half | median | worst |
|---|---|---|
| `intra_pred_common` shims only (SAD raw) | **+0.57%** | +4.02% |
| `sad_common` shims only (intra raw) | **+16.76%** | +78.16% |

So `intra_pred_common` stayed swapped and `sad_common` was unswapped (`11f82d41`).

**Why the microbenchmark said 0.82-1.33x.** It drove the kernels at a **1984-byte
stride over a 190 KB working set**. Every row access was a cache miss, and the safe
kernels' extra per-row work hid entirely behind the misses. The identical benchmark at
a 64-byte stride over 4 KB — L1-resident, which is what a motion search is — agrees
with the encoder:

| shape | raw | safe, 190 KB set | safe, 4 KB set |
|---|---|---|---|
| Sad4x4 | 2.75 | 4.65 (1.02x*) | 4.26 (**1.55x**) |
| Sad8x8 | 4.20 | 11.7 (1.37x*) | 7.03 (**1.68x**) |
| Sad8x16 | 6.98 | 16.4 (1.02x*) | 12.45 (**1.79x**) |
| Sad16x8 | 6.86 | 19.3 (1.58x*) | 7.02 (1.02x) |
| Sad16x16 | 12.48 | 19.0 (0.81x*) | 12.45 (1.00x) |
| SadFour8x8 | 12.63 | 43.0 (2.03x*) | 25.28 (2.00x) |
| SadFour16x16 | 38.32 | 67.2 (1.29x*) | 47.00 (1.23x) |

\* ratios against that column's own raw timings, which the large working set inflated
too — the point is that the ratio is meaningless when the memory system dominates
both sides.

**The shape of the remaining cost, from the corrected instrument.** It is **per row
and independent of width**: through the safe kernel, 4x8, 8x8 and 16x8 all take
~7.0 ns, against 4.0 / 4.2 / 6.8 ns raw. The per-sample arithmetic is free — it
vectorises — and what is left is bounds and iterator work once per row, which only the
16-wide shapes have enough samples to amortise. Any fix has to attack **per-row
overhead**, not the inner loop. Two things already measured and rejected on the
corrected instrument: rolling the row offset instead of computing it (worse), and
`u8::abs_diff` for the per-sample term (no change — LLVM already emitted it).

**`row_windows` is not the culprit and stays.** Re-measured L1-resident against the
per-row `PlaneCursor::row` walk it replaced, it wins on twelve of fourteen shapes and
wins large on the four-point ones (1.2-2.0x against 2.1-3.3x). It was the right change
made for the wrong reason, and `mc.rs` was correctly not refitted onto it.

**Instrument rule this establishes, binding on T6-T9:** a kernel microbenchmark must
run at a working set the real caller has. Size it from the caller, state the size next
to the numbers, and where the caller's residency is not obvious, **measure both** — a
kernel that is at parity memory-bound and 1.7x L1-resident is 1.7x in an encoder.

### T8 — `processing/{vaacalc,adaptive_quantization}.rs` — landed, and the session's instrument lesson

Six kernels, all swapped. The headline is that **five of six are at parity or
faster** and the family landed inside budget, but the route there is worth more than
the numbers: this session made *two* microbenchmark errors of the exact kind session
C's rules were written to prevent, and caught both only because the end-to-end bench
and the arithmetic disagreed with the microbench.

**Kernel bodies**, safe against the raw ones recovered from commit A with `git show`,
interleaved, medians, at the caller's working set (a whole picture per frame — this
family is streaming, so the large working set is the honest one here, unlike T5):

| kernel | 1080p | 720p | VGA | QVGA |
|---|---|---|---|---|
| `VAACalcSad` | 0.86x | 0.79x | 0.82x | 0.86x |
| `VAACalcSadVar` | 0.86x | 0.86x | 0.85x | 0.85x |
| `VAACalcSadSsd` | 0.69x | 0.68x | 0.68x | 0.69x |
| **`VAACalcSadBgd`** | **1.33x** | **1.39x** | **1.43x** | **1.47x** |
| `VAACalcSadSsdBgd` | 0.97x | 0.97x | 0.97x | 0.97x |
| `SampleVariance16x16` (per picture) | 1.04x | 1.02x | 1.04x | 1.05x |

**What made the fast ones fast, in the order it mattered.** The first version measured
**2.51x** at 1080p. Disassembly (first, not after two guesses) found 16
`slice_index_fail` sites: `cur[base..base + 8]` re-checks per row per plane, 64 per
macroblock — T5-sad's mechanism exactly. Two changes fixed it:

1. **Read a 16-wide row once and split it in registers** instead of walking each 8x8
   quadrant separately the way the C++ does. Halves the checks (32 per macroblock) and
   walks the macroblock in one sequential pass. 2.51x → 1.82x.
2. **Trim each plane to exactly the span the eight rows read, before the row loop.**
   Handed an open tail (`&cur[origin..]`) LLVM cannot relate `k * stride` to the slice
   length and re-checks every row; handed a window it knows is `7 * stride + 16` long,
   it folds them. 1.82x → **1.02x**, and 0.84-0.88x at the smaller sizes.

Step 2 is the transferable one and it is **the technique D-perf-2 should try on
SAD**: same shape (fixed-size window, runtime stride, per-row checks that will not
fold), and it is not one of the three things T5 already measured and rejected.

**The one kernel that lost, and why.** `VAACalcSadBgd_c` sits at 1.44x. Not bounds
checks — the disassembly says the raw kernel issues 16 `uabd` and 16 `umax` where this
one issues 8 and 8. `BGD`'s two extra accumulators (a signed sum and a running
maximum) are **per quadrant** and cannot merge across a row, so each 8-sample half
fills half a vector register and the loop vectorises at half width. With `SQDIFF` and
`VAR` also on there is enough other arithmetic to hide it (`SsdBgd` 0.97x); with none
of them on, all 16 samples go through one `uabd.16b` (`Sad` 0.86x). Only the middle
case loses. Passing the statistics by value instead of `&mut` was tried and measured
**worse** (1.52x against 1.33x) — do not retry it. The real fix is a second,
quadrant-shaped walk selected on the `BGD` const, which is left undone here and is a
live option for T7's similarly shaped kernels.

**End to end**, `c_vs_rust_bench` with `FFMPEG` set, commit A (raw) against commit B
(swapped), binaries kept on disk and interleaved, 3 pairs, Rust rows:
**median +3.48%**, worst usable row +8.53% (720p SMPTE Bars, 4 threads), 6 usable rows
over +5%, none over the §7.4 ceiling of 10%.

#### Two instrument errors, both caught, both worth not repeating

**1. After a swap, the "raw" side of a microbenchmark is the shim.** The six-kernel
table above was first produced *after* commit B, calling `vaa::VAACalcSad_c` as the
raw side — which by then was a shim onto the safe kernel. Every row read 0.99-1.07x,
which looked like a clean result and was a function compared with itself. The raw
bodies have to come from `git show` of the pre-swap commit, exactly as T4's log says.
**Measure bodies before swapping, or recover them explicitly; never call the old name
afterwards and believe it.**

**2. Calibrate the end-to-end bench with a null run before disbelieving it.** The
+3.48% was initially suspected of being noise, on the reasoning that the whole VAA
walk costs 0.11 ms at 1080p and cannot produce the observed +0.29 ms. Running the
**same binary in both slots** through the identical harness settled it:

| | median | max | rows > +5% | rows > +10% |
|---|---|---|---|---|
| null (identical binary both sides) | **+0.00%** | +1.81% | 0 / 30 | 0 |
| T8 swap | +3.48% | +29.52% | 8 / 30 | 2 |

So the bench's noise floor is about ±2% and **the +3.48% is real**. The suspicion was
wrong and the null run is what proved it — cheap, decisive, and now the standard step
before attributing a bench reading to noise. It also independently re-condemned
**Spatial Ramps**, which moved **-38%** between two runs of the same binary: that row
cannot detect anything and both of T8's ">10%" rows are it.

A residual remains honestly unexplained: the measured kernel deltas do not add up to
+3.48% end to end (`Bgd` contributes about +0.07 ms/frame at 1080p against an observed
+0.31 ms). It is recorded rather than resolved. Candidates not chased: code layout
differences between the two binaries, and which walker the rate-control flags actually
select frame by frame — `bEnableBackgroundDetection` and `bEnableAdaptiveQuant` both
default **on** (`encoder/param_svc.rs:351-352`), so `Bgd`/`SsdBgd` are live and
`VAACalcSad_c` — the only kernel the file's header used to claim was reachable — may
be the one that mostly is not.


### T6 — deblocking + the F1 surgery + border expansion — landed under D-perf-4's protocol

The first family group measured under §7.4 v3: **one interleaved pair per bench per
commit B**, binaries kept on disk, `FFMPEG` set, null run only where a "noise" claim
needed it. No microbenchmarks were built (D-perf-4 makes them diagnostic; nothing here
surprised).

**Deblocking (`common/deblocking_common.rs`, 13 shims; A `5756ad75` → B `64633e5f`),
decode bench Rust rows (each side internally best-of-3):**

| stream | A (raw) | B (swapped) | delta |
|---|---|---|---|
| Constrained Baseline (CAVLC) | 2.375 ms | 2.560 ms | **+7.79%** |
| Main (CABAC, B-frames) | 6.118 ms | 6.274 ms | +2.55% |
| High (CABAC, 8x8) | 6.173 ms | 6.317 ms | +2.33% |

The shape is T4's decode-side entry again: fixed per-edge scaffolding (span
arithmetic, one or two `from_raw_parts`, cursor construction, per-access checked
indexing against a runtime `step_x`/`step_y`) levied per filtered edge, so the
fastest stream — CAVLC at 2.4 ms/frame — pays the largest fraction. Encoder side:
median **-0.11%** over 28 usable rows (max +1.62%), i.e. flat, exactly as the
consumer topology predicts — the encoder runs its own kernel copies in
`encoder/deblocking.rs` and installs nothing from this module. Tripwire arithmetic
at commit B: cumulative decode ≈ +16.0 / +9.8 / +9.3% (T4 + this), worst stream well
under +25%, no single-family stream reading over 15% → swap-and-ledger.

**The F1 surgery (`d878916b`, encoder-only):** median **+0.36%** over 28 usable rows
(range -1.68 .. +5.10%). The same session's null run (below) puts same-binary row
movement at ±5.4%, so this is noise-level; recorded, not ledgered — it adds no shim
and no scaffolding, it replaces five 32-byte casts with typed stores.

**Expansion (`decoder_core.rs` shims onto `common/expand_pic.rs`, 2 shims;
`d878916b` → `9c32f890`):** decode **+0.97 / +0.08 / +0.53%**; encoder median +1.55%
(range -3.09 .. +10.45%). The +10.45% outlier is QVGA YUV Space [4t] — and the null
run (same `9c32f890` binary, two passes, this session) moved **that same row -5.41%**:

| | median | min | max |
|---|---|---|---|
| null (identical binary, this session) | -1.27% | -5.41% | +4.36% |
| expand swap | +1.55% | -3.09% | +10.45% |

Today's floor is ±5% on the tiny QVGA rows and ~±1.3% on medians — noticeably worse
than session D's ±1.81% null (the machine had been building for hours; session A saw
the same drift pattern). Verdict: the expand encoder reading is indistinguishable
from noise, which is what a once-per-reference-picture kernel should read as. The
decode side's ~+1% on CAVLC is at the floor's edge and is carried in the ledger row
for honesty rather than argued away.

Cumulative after T6: decode ≈ **+17 / +10 / +10%**, encoder ≈ **+11%** (T4 + T8;
T6's encoder contributions are all inside the floor). All under the +25% tripwire.

### T7 part 1 — forward transform/quant/scan + encoder recon IDCT + SATD (session F, D-perf-4 protocol)

Session F's null run (same control binary both slots, fresh per R-l): median
**+0.87%**, min -3.10%, max +2.90%, zero rows over 5% — the quietest floor of the
three sessions that have measured one.

**F-1 `encoder/encode_mb_aux.rs` (21 shims; A `f233d506` → B `b1fb7448`),
one interleaved pair per bench:** decode **+0.48 / +0.26 / +0.35%** (flat — the
decoder installs nothing from this file). Encoder **+2.10% median, +9.86% worst
usable, 9 rows over +5%, none over +10%**. The shape is T4's per-call mechanism on
the encoder side: the flat-content synthetic rows (SMPTE/PAL/RGB/YUV bars at
QVGA-VGA, +5-10%) encode fastest and pay the largest fraction of fixed per-block
shim cost (transform+quant runs per coded block), while the content-heavy rows
(Mandelbrot, Testsrc) read -6..+2%. No microbenchmark built: the number matched the
known mechanism (D-perf-4 makes it diagnostic-only). Tripwire: encoder cumulative
≈ +11% + 2.1% ≈ **+13% median, under +25%** → swap-and-ledger.

**F-2 `encoder/decode_mb_aux.rs` + the five raw bodies in `svc_encode_mb.rs`
(9 shims; A `fc92dab0` → B `55e6d7fe`), one pair:** decode +0.61% median, encoder
**+0.82% median, max +3.52%, zero rows over 5%** — noise-level both benches,
exactly the pilot-shaped expectation. Recount recorded: the C++ family is 9 leaf
kernels, not the 6 the finishing brief carried.

**F-3 `encoder/sample.rs` SATD: no measurement — prove-and-park by decision**
(D-perf-2's sad-class numbers are the projection; §Parked row below).

**The Spatial Ramps caveat, kept honest:** the condemned row read **+226%** at
F-1's pair (0.111 → 0.362 ms) and the readings are *consistent per binary* across
runs today (control-class binaries 0.11-0.15 ms; the F-1 B binary 0.345-0.362 ms
over three separate passes; the F-2 B binary 0.223 ms twice — i.e. ≈2-3x against
control, direction stable). That is unlike the row's historical same-binary ±38%
chaos, and gradients content is exactly the all-skip path where per-block quant
scaffolding is the whole cost. The row stays excluded from every verdict per R-l,
but this observation is recorded as **data for Phase 4a's checkpoint**: if
direct dispatch recovers it, it was the per-call mechanism at its purest; output
was bit-identical throughout.

Entries here are temporary regressions attributable to strangler-shim scaffolding,
carried under §7.4's three conditions (kernel bodies at parity by microbench; overhead
demonstrated fixed-per-call; deleting phase named). The ≤10%-per-stream hard ceiling
is judged over the *sum* of live entries. Phase 4 re-measures every entry (direct
dispatch makes shims inlinable); Phase 5 closes them with the shims.

| family | entered | deficit | body evidence | deleting phase | Phase 4 checkpoint | closed |
|---|---|---|---|---|---|---|
| T4 `common/mc.rs` (28 shims) | 2026-08-08, D-perf-1 | decode +8.2% / +7.2% / +7.0%; **encoder +7.6% median, +16.7% worst** (measured 2026-08-09, see below) | centre kernels 0.88–0.99x; 8x8 chroma copy overhead 7 ns fixed/call (§Phase 2 T4) | Phase 5 | **encoder RECOVERED, decode DOWNGRADED** (2026-08-10). De-virtualized `4a-1`: encoder **-4.84% median** on its own pair, decode **+0.41%** (inside floor). Encoder call sites pass literal block sizes so the shim inlines and folds; `BaseMC` takes them as parameters, so the decode side cannot fold and `#[inline]` on a ~1300-instruction function is declined. Decode half moves to Phase 5 per D-perf-3's fallback | — |
| T8 `processing/*` (6 shims) | 2026-08-09 | encoder +3.5% median, +8.5% worst usable | five of six bodies 0.69–1.04x; `VAACalcSadBgd_c` 1.44x (§Phase 2 T8) | Phase 5 | **not de-virtualized in 4a** — its dispatch is the encoder's VAA members, left for 4b/Phase 6. Carried into the -5.11% aggregate but not separately attributable | — |
| T6 deblocking `common/deblocking_common.rs` (13 shims) | 2026-08-10, under D-perf-4 | decode +7.8% / +2.6% / +2.3%; encoder flat (-0.1% median — not a consumer) | none taken: D-perf-4 makes the microbench diagnostic-only and the shape matched T4's known per-call mechanism; Phase 4 re-measures | Phase 5 | **DOWNGRADED to Phase 5** (2026-08-10). De-virtualized `4a-3` (12 slots, 22 sites): decode **-0.53% median** on its own pair. Same mechanism as T4's decode half — the shim cost is span arithmetic over a *runtime stride*, which direct dispatch does not make constant. Caller conversion is what fixes it | — |
| T6 expand `decoder_core.rs` (2 shims) | 2026-08-10, under D-perf-4 | decode +1.0% / +0.1% / +0.5% (at the noise floor's edge); encoder inside the null floor (§Phase 2 T6) | once-per-reference-picture kernel; body is the same memcpy/memset calls | Phase 5 | **not de-virtualized in 4a** (scope cut, §Phase 4a). Deficit is at the noise floor and the kernel runs once per reference picture, so it was the correct row to drop | — |
| T7 `encoder/encode_mb_aux.rs` (21 shims) | 2026-08-10, under D-perf-4 | encoder +2.1% median, +9.9% worst usable (flat-content rows +5-10%); decode flat (+0.3-0.5%, not a consumer) | none taken: D-perf-4 diagnostic-only, shape matched T4's per-call mechanism (fixed per-block cost, largest fraction on fastest content); Spatial Ramps observation recorded in §Phase 2 T7 part 1 | Phase 5 | **not de-virtualized in 4a** (scope cut) — its dispatch is `InitCoeffFunc`'s members. Its flat-content rows are the ones that moved most in the aggregate, so it is the first candidate for the next de-virtualization pass | — |
| T7 `encoder/decode_mb_aux.rs` + `svc_encode_mb.rs` recon (9 shims) | 2026-08-10, under D-perf-4 | encoder +0.8% median / decode +0.6% median — both inside the session's null floor | noise-level; nothing to isolate | Phase 5 | **closed as noise** (2026-08-10). Both figures were inside their session's null floor when entered and remain so; there is no deficit here to recover. Row retained for the audit trail, not as a debt | **yes** |
| T7 `encoder/get_intra_predictor.rs` (26 shims) | 2026-08-10, under D-perf-4 | encoder +0.42% median / max +2.69%, decode flat — **inside the session's null floor** (median +0.00%, max +2.50%) | none taken: per-mode-per-MB call density, T3's wash reproduced on the encoder side | Phase 5 | **closed as noise** (2026-08-10), same basis as the recon row: entered inside its null floor, still inside it | **yes** |
| T9 `encoder/deblocking.rs` straggler (1 shim; 8 wrappers re-exported) | 2026-08-10, under D-perf-4 | encoder +0.59% median / **+4.33% worst**, decode unchanged | small but real: the four Mandelbrot rows move together above the null floor — the encoder now pays the per-edge shim cost the decoder has paid since T6 | Phase 5 | **partially recovered** (2026-08-10): the encoder's deblocking still dispatches through its own table (4b/Phase 6), but the four Mandelbrot rows that identified this deficit now read -0.13% to -1.82% at the phase exit. Carried in the aggregate | — |

### T7 part 2 + T9 straggler — encoder intra predictors and the encoder's deblocking duplicate (session G)

Null floor, fresh (**S2**, same binary both slots): median **+0.00%**, max **+2.50%**,
zero rows over 5%. `Spatial Ramps` moved **-10.7 / -11.5%** on that same-binary run, and
**-24%** in the G-1 pair — it re-earned its exclusion twice in one session.

**G-1 `encoder/get_intra_predictor.rs` (26 shims; A `6e0009d3` → B `cbc18d94`), one
interleaved pair per bench:** encoder **+0.42% median, max +2.69%, zero rows over 5%**
— inside the null floor. Decode **+0.58 / -0.05 / +0.19%**, flat, as it must be: the
decoder installs nothing from this module. T3's wash reproduced on the encoder side,
which is what the call density predicted (per-mode-per-macroblock, an order less often
than SAD per candidate). Tripwire: encoder cumulative ≈ +13% + 0.4 ≈ **+13.4%, under
+25%** → swap and ledger. No microbenchmark: nothing surprised.

**G-2 `encoder/deblocking.rs` (T9 straggler; A `b3da21e1` → B `ee8735df`), one pair:**
encoder **+0.59% median, max +4.33%, zero rows over 5%**; decode **-0.38% median**
(unchanged — the decoder already ran these shims, and a moved decode number would have
meant the re-export changed the wrong caller). The four Mandelbrot rows move together
at +3.7..+4.3%, above the null floor's +2.50% max, so this reads as a small **real**
cost rather than noise: the encoder now pays the per-edge shim scaffolding the decoder
has paid since T6, on the content that deblocks most. T6's decode +7.8% mechanism at a
fifth the size. Cumulative ≈ **+14%, under +25%** → swap and ledger.

---

## Phase 2 exit — the cumulative measurement, and what it says about the ledger

**Measured 2026-08-10 at Phase 2's close: the phase's control commit `afcdd785`
against `a6361ee3`, both bench binaries built and kept on disk, three interleaved
pairs per bench, medians (S1).** This is the one place the heavyweight protocol still
runs, and it is the first time the ledger has been checked against a direct
start-to-end reading rather than believed as a running sum.

Exit null floor (S2, same binary both slots, immediately after): median **+0.00%**,
max **+4.0%** excluding `Spatial Ramps` (which read +13.8% / +11.9% against itself and
is excluded from every verdict).

### Decoder — `decode_1080p_bench`, 3 pairs

| stream | Phase 2 start | Phase 2 end | delta | ledger predicted |
|---|---|---|---|---|
| Constrained Baseline (CAVLC, no B-frames) | 2.218 ms | 2.586 ms | **+16.59%** | ≈ +17% |
| Main (CABAC, B-frames) | 5.736 | 6.346 | **+10.63%** | ≈ +10% |
| High (CABAC, B-frames, 8x8) | 5.813 | 6.386 | **+9.86%** | ≈ +10% |

### Encoder — `c_vs_rust_bench`, 3 pairs, 30 rows

**Median +14.73%**, min +5.40%, max **+33.3%** usable (+60.0% on `Spatial Ramps`,
excluded). All 30 rows over +5%; 19 over +10%. Ledger predicted ≈ +14% median.

| row class | delta |
|---|---|
| flat-content QVGA/VGA synthetics (SMPTE, PAL 75%, RGB, YUV Space) | +25 .. +33% |
| 720p/1080p SMPTE Bars | +14 .. +17% |
| Testsrc 1080p | +9 .. +11% |
| Mandelbrot, all sizes | +5.4 .. +8.2% |
| Moving Box, High-Contrast QVGA | +7.5 .. +13% |
| `Spatial Ramps` (excluded) | +58 .. +60% |

**The two instruments agree.** Six ledger rows summed to ≈ +14% on the encoder and
≈ +17/+10/+10% on the decoder; the direct measurement says **+14.73%** and
**+16.6/+10.6/+9.9%**. That is the ledger validated as an instrument, not just as
bookkeeping — every per-family reading was taken with one pair, and their sum survives
a three-pair end-to-end check. Phase 4a's checkpoint can therefore read recovery row by
row and trust the arithmetic.

**The shape is the mechanism, stated once for the phases that inherit it.** The
gradient runs cleanly from Mandelbrot (+5-8%: dense, high-entropy content where real
coding work dominates) to the flat synthetics (+25-33%: almost nothing to code, so the
fixed per-block scaffolding *is* the frame time) to `Spatial Ramps` (+58%: the all-skip
path, pure scaffolding). The deficit is **per-call, not per-sample** — which is exactly
the claim Phase 4a's direct dispatch is supposed to cash, and exactly why the flat rows
are the ones to watch when it does.

**Against the budget:** §7.4's ≤10%-per-stream hard ceiling was retired by **D-perf-4**
(2026-08-09) in favour of the +25%-median-cumulative tripwire. The encoder's +14.73%
median and the decoder's +10.63% are both under it, so nothing parks retroactively.
The decode Constrained Baseline row (+16.6%) and 19 encoder rows are over the old
ceiling; that is the ledgered, deliberate state D-perf-4 chose, not a breach.

### Per-family delta table, as landed

| family | shims | encoder | decode | note |
|---|---|---|---|---|
| T2 pilot `decoder/decode_mb_aux.rs` | 4 | — | **-1.3 .. -1.8%** (faster) | the pilot verdict that licensed the cursor API |
| T3 `decoder/get_intra_predictor.rs` | 42 | — | wash | the designated worst case, and it was not one |
| T4 `common/mc.rs` | 28 | +7.6% med / +16.7% worst | +8.2 / +7.2 / +7.0% | the largest single row; bodies 0.88-0.99x |
| T5-intra `common/intra_pred_common.rs` | 2 | +0.57% | — | |
| T5-sad `common/sad_common.rs` | 0 | **parked** | — | 14 kernels proven, unswapped |
| T6 deblocking `common/deblocking_common.rs` | 13 | flat | +7.8 / +2.6 / +2.3% | |
| T6 expand `decoder_core.rs` | 2 | flat | +1.0 / +0.1 / +0.5% | |
| T8 `processing/*` | 6 | +3.5% med / +8.5% worst | — | `VAACalcSadBgd_c` body 1.44x, mechanism recorded |
| T7 F-1 `encoder/encode_mb_aux.rs` | 21 | +2.10% med / +9.86% worst | flat | |
| T7 F-2 encoder recon IDCT | 9 | +0.82% | +0.61% | noise-level |
| T7 F-3 `encoder/sample.rs` SATD | 0 | **parked** | — | 7 kernels proven, never installed |
| T7 G-1 `encoder/get_intra_predictor.rs` | 26 | +0.42% | flat | inside the null floor |
| T9 G-2 `encoder/deblocking.rs` | 1 | +0.59% / +4.33% worst | flat | the straggler; 13 raw bodies deleted |
| **total, measured end-to-end** | **154** | **+14.73% median** | **+16.6 / +10.6 / +9.9%** | |

---

### The encoder-side ceiling is breached, and it needs a decision (2026-08-09)

**D-perf-1's addendum asked for T4's encoder contribution to be isolated. It is
+7.6% median and +16.7% at worst, and that breaches §7.4's 10%-per-stream hard
ceiling.** Measured commit A `46053993` (mc.rs raw) against commit B `ea52b387`
(mc.rs swapped), binaries kept on disk and interleaved, 3 pairs, Rust rows, with the
same-binary null run putting the floor at ±2%:

| stream | thr | A (raw) | B (swapped) | delta |
|---|---|---|---|---|
| 1080p SMPTE Bars | 4 | 2.918 | 3.405 | **+16.69%** |
| 320x240 PAL 75% | 1 | 0.057 | 0.066 | **+15.79%** |
| 320x240 YUV Space | 4 | 0.059 | 0.067 | **+13.56%** |
| 640x480 SMPTE Bars | 1 | 0.219 | 0.248 | **+13.24%** |
| 1080p SMPTE Bars | 1 | 2.899 | 3.250 | **+12.11%** |
| 320x240 SMPTE Bars | 1/4 | 0.058 | 0.065 | **+12.07%** |
| 1080p Testsrc | 1 | 4.318 | 4.654 | +7.78% |
| 1080p Mandelbrot | 1 | 10.270 | 10.834 | +5.49% |
| 720p Mandelbrot | 1 | 5.113 | 5.237 | +2.43% |

Median **+7.57%** over the 28 usable rows (Spatial Ramps excluded — the null run
moved it -38%), worst **+16.69%**, **11 usable rows over the 10% ceiling**.

Three things follow, none of which this session decided:

1. **The cumulative encoder deficit is roughly +11%**, because T8's +3.5% was measured
   on top of an already-swapped `mc.rs`. §7.4 judges the ceiling over the *sum* of live
   entries, and the sum is over.
2. **This does not change T4's ledger eligibility.** Its bodies are still 0.88–0.99x
   and the residual is still fixed per-call scaffolding that Phase 5 deletes with the
   shims. What changed is the size, and the reason it is bigger here than on the decode
   side is that motion estimation calls MC far more times per frame than decoding does
   — a fixed per-call cost is levied per *call*, and the encoder makes many more.
3. **It is a phase decision, like D-perf-1 was, and it is Eugene's.** The options are
   the same three: carry a larger deficit to Phase 5 with the ceiling explicitly raised
   for encoder streams and the reasoning recorded; revert T4 and leave `mc.rs` raw
   until Phase 5 converts its callers; or bring Phase 4's direct-dispatch checkpoint
   forward for `mc.rs` specifically, since direct calls are what make these shims
   inlinable and the per-call cost is exactly what inlining removes.

   **DECIDED 2026-08-09 — D-perf-3 (plan §7.4): option three, boxed, with option two
   as the pre-authorized same-session fallback.**

   **SUPERSEDED same day by D-perf-4 (plan §7.4 v3, Eugene's direction): safety
   first.** The ceiling this section reports a breach of is retired; the numbers
   above stay as the record of what the swap costs, the ledger row stays open, and
   recovery moved to the checkpoints — the mc.rs direct-dispatch experiment is now
   **Phase 4's first task**, shim deletion clears scaffolding at Phase 5, and the
   Phase 9 perf pass owns whatever remains. Interim rule from here: swap-and-ledger
   by default; a family that would push any stream's cumulative past **+25% median**
   parks instead.

What is *not* open is whether a cleverer kernel fixes it — T4 measured three structural
mitigations and rejected all three, and its bodies are already at or under parity.

**The instrument lesson underneath this is the same one session C paid for.** T4 landed
in session B, was judged against the decode bench alone, and was carried under a
decision that assumed the encoder side looked similar. It did not, and nobody could
have known, because `FFMPEG` was unset and `c_vs_rust_bench` had been printing SKIP for
three families. One environment variable hid a ceiling breach for two sessions.

**T5-sad was considered for this ledger and rejected**, which is worth recording
because it is the criteria doing their job. Condition (a) is that the paired
microbenchmark shows kernel *bodies* at ≤1.05x, and once the microbenchmark was
corrected for working set it showed 1.0–2.0x — the cost is in the safe kernel, not in
the shim, so Phase 5 would not delete it. Condition (b) fails for the same reason: the
overhead is per row, not fixed per call. A deficit that no later phase removes is not a
deficit, it is a regression, and the family was unswapped instead (`11f82d41`). The
distinction between this and T4 is exactly what the two-ledger split was written for.

### Parked families (proven, unswapped; §7.4 "parked" state)

Safe kernels in-tree and differentially proven, shims unswapped because body cost at
the shim boundary exceeds parity. Each entry names its re-attempt point. This table
must also be empty by Phase 5's exit — parked families close by re-landing (technique
found), by the Phase 4 direct-dispatch checkpoint, or by their callers converting.

| family | parked | body evidence | blocking mechanism | re-attempt point | closed |
|---|---|---|---|---|---|
| T5-sad `common/sad_common.rs` (14 shims unswapped, `11f82d41`) | 2026-08-09 | ~7.0 ns/call regardless of block shape (4x8 = 8x8 = 16x8), L1-resident; encoder stream cost +16.8% median / +78% worst | per-row bounds + iterator work on tiny fixed-size blocks with runtime stride; per-sample arithmetic already free; `row_windows`, rolled offsets, `abs_diff`, and now T8's exact-span shape all measured null | **Re-attempted at Phase 4a's checkpoint 2026-08-10 on a rebuilt harness — stays parked, second dated verdict** (1.41x-4.94x across the seven shapes; §Phase 4a). Next: caller conversion (ME, Phase 6.3), or a real slices-and-offsets kernel — that lead is still untested, see §Phase 4a for why the cheap probe was invalid | — |
| T7-satd `encoder/sample.rs` (7 safe kernels, never installed; `1383acb5`) | 2026-08-10 | none taken — parked by projection, not measurement: D-perf-2's sad-class bodies 1.39-2.97x and the T5 swap's +16.8% median, onto an encoder cumulative ≈ +13%, cross the +25% tripwire | same class as T5-sad (tiny blocks, per-candidate ME calls) with a Hadamard butterfly on top — strictly more work per call than SAD, which already failed parity | **Phase 4a re-attempt NOT DISCHARGED.** The SAD verdict is a strictly-easier lower bound (SATD = SAD + a Hadamard butterfly) so the park holds a fortiori, but this family still has no measurement of its own. Owed by whoever next opens the box; alongside T5-sad at caller conversion (Phase 6.3) | — |

**Phase 2 closed 2026-08-10 with both rows open, by design.** Neither family is a
Phase 2 failure: T5-sad spent the phase's one sanctioned attempt and T7-satd was parked
on that measurement's projection rather than its own. **Phase 4a's checkpoint is their
re-attempt point and it is now the immediately next phase** (D-seq-1), which is the
shortest the park was ever going to be. Two things go with them: the
slices-and-offsets lead below, and the reason re-landing is a *safety* goal and not
only a perf one — parked raw code is where latent UB sits, and F10 was found in this
very family three separate times.

#### D-perf-2's one attempt, and its verdict (2026-08-09)

**Verdict: parked. T7's SAD/SATD families go prove-and-park from the start.** This is
exit state (b) of the two the decision allowed, and the box is now closed — a third
investigation is out of scope for the phase.

The candidate was the shape that had just taken T8's whole-picture walk from 2.51x to
1.02x, and it was a real candidate rather than a guess: same problem (fixed-size
window, runtime stride, per-row checks that will not fold) and **not** one of the three
things T5 had already measured and rejected. Trim the block to its exact span once
(`(H-1) * stride + W`), then index rows directly at `k * stride` instead of walking
them with `chunks()`, which is what `PlaneCursor::row_windows` does.

Measured L1-resident (64-byte stride over 4 KB, the residency a motion search has),
raw against the live pointer kernels — T5-sad being unswapped means those are genuinely
raw. In the framing **most favourable to the candidate** — plain slices and offsets,
no cursor construction at all — it came in at **1.39x to 2.97x** across the seven
shapes. Better than the in-tree kernel in the same harness, and nowhere near the
≤1.05x body-parity bar the swap requires.

Two things worth carrying, and one caveat that limits how far to trust the numbers:

- **`PlaneCursor::new` is itself a significant per-call cost at this size.** Making the
  candidate pay the same two constructions the in-tree kernel pays moved 4x4 from 1.61x
  to **4.93x** and 8x8 from 2.29x to 3.24x. For kernels whose whole body is a couple of
  nanoseconds, validating a cursor twice per call is a large fraction of the work. That
  suggests a direction genuinely outside T5's rejected set: let the shim hand these
  kernels **slices and offsets** rather than cursors. It is a convention change (the
  phase's rule is that plane-reading kernels take cursors), so it belongs to whoever
  reopens this at the Phase 4 checkpoint, not here.
- **The caveat: this harness is not sound enough for its absolute numbers to enter the
  record.** It reads the in-tree `sample_sad` at 2.9-8.7x where T5's harness read
  1.55-1.68x and 1.00-1.02x, and raw `Sad8x4` moved 6.49 ns → 2.18 ns between two runs
  of the same binary. At ~2 ns per call it cannot resolve the small shapes. The verdict
  rests only on the robust part: **nothing measured came near parity, in any framing,
  including the one that gave the candidate every advantage.** Anyone reopening this
  should rebuild the harness before trusting a number from it.

## Phase 4a — the recovery checkpoint (2026-08-10)

**Measured at Phase 4a's close: `2f65a765` (Phase 2's exit) against `25a8e287`,
both bench binaries built and kept on disk, three interleaved pairs per bench,
medians (S1), via the new `rust/tools/perfpair.py`.**

Session null floor (S2, measured at session start, same binary both slots):
decode median +0.35% / max +0.40%; encoder median **+0.00%**, max +1.50%, zero
rows over 5% — the tightest floor the project has recorded. `Spatial Ramps` read
-13.8% / -12.4% against itself and re-earned its exclusion.

### The headline

| bench | median | min | max | rows slower |
|---|---|---|---|---|
| **encoder** `c_vs_rust_bench`, 28 usable rows | **-5.11%** | -10.39% | -0.13% | **none** |
| **decode** `decode_1080p_bench`, 3 rows | **-0.19%** | -0.47% | +1.07% | — |

**Every one of the 28 usable encoder rows moved faster or flat. Not one
regressed.** That sign consistency is itself the strongest evidence the result is
real: a ±1.5% floor does not produce 28 consecutive negative rows.

`Spatial Ramps`, excluded from every verdict and *still* the most sensitive
instrument here, went **-48.4% / -47.6%** — 0.32 ms to 0.165 ms, near enough a
halving. Session F measured this row at +226% and Phase 2's exit at +58/+60%
cumulative. Gradient content is the all-skip path, per-block scaffolding at its
purest with almost no real coding work to dilute it, and the brief predicted that
if direct dispatch showed up anywhere it would show up loudest here. It did. It
still cannot decide anything.

### Cumulative position

| | Phase 2 exit vs Phase 2 start | Phase 4a delta | **cumulative now** |
|---|---|---|---|
| encoder, median | +14.73% | -5.11% | **≈ +8.9%** |
| decode Constrained Baseline | +16.59% | +1.07% | ≈ +17.8% |
| decode Main | +10.63% | -0.47% | ≈ +10.1% |
| decode High | +9.86% | -0.19% | ≈ +9.6% |

The encoder is back under +10% for the first time since T4 landed. The decode
side is where it was.

### What the checkpoint actually established

**Direct dispatch recovers per-call scaffolding only where the caller supplies
constant dimensions.** That is the phase's finding, it was measured three times
on two families in both codecs, and it explains the entire encoder/decoder split:

- **Encoder call sites name the shims with literal block sizes** (`16, 16` /
  `8, 8`), so inlining folds the shim's span arithmetic and its `from_raw_parts`
  against them. `mc.rs` alone measured **-4.84% median** the moment its fifteen
  sites became direct calls.
- **Decoder call sites do not.** `BaseMC` takes `iBlkWidth`/`iBlkHeight` as
  parameters — constant at its ~40 call sites, runtime inside it — and at ~1300
  instructions it is far past any inlining threshold. Deblocking is the same
  shape with a runtime *stride*. Measured, not assumed: `#[inline]` on `BaseMC`
  read -0.35% and `#[inline(always)]` on `McChroma_c` -0.71%, both inside the
  floor, both reverted. Disassembly confirms `McLuma_c` does inline into `BaseMC`
  while the chroma pair stays out of line.

So D-perf-3's fallback applies **to the decode half only**, and the two decode
ledger rows are downgraded to Phase 5 rather than left promising a recovery that
has now been tested and did not arrive. Phase 5 is the right owner on the merits,
not as a consolation: converting callers is exactly what makes those dimensions
static, and it deletes the shims anyway.

### Scope actually covered, and what was cut

De-virtualized: `SMcFunc` (6 slots, 15 sites), the decoder's `SDeblockingFunc`
(12 slots, 22 sites), and `pfMdCost`/`pfMeCost` (F13's fourth site → enum).

**Cut, deliberately, to keep the checkpoint whole** (the brief's own instruction
if the phase ran short): the decoder intra-pred arrays, `sBlockFunc`, expand, the
`decode_slice.rs` cache-fill transmutes, and the encoder's ~55 CPU-dispatch
members including `WelsInitSampleSadFunc`. The intra-pred arrays are the one
cut worth explaining: they dispatch on *mode*, which comes from the bitstream,
so they are not the CPU-dispatch case at all — they need `enum` + `match`, and
T3/T7-G1 measured that family as a wash in both codecs, so the perf payoff is
near zero and the payoff is unsafe-surface only.

The 23 `transmute`s are therefore untouched and the ratchet's `transmute` count
is unchanged at 23. That is the phase's main unfinished business.

### Parked families — second verdict, and it is unchanged

**Both stay parked. Dated 2026-08-10, and this time on a rebuilt instrument.**

The brief's precondition was to rebuild the harness before trusting either;
`benches/sad_bodies_bench.rs` is that rebuild, written against S1/S3/S4 with each
rule's reason in its module docs. Ratios now reproduce within ~3% across runs,
against a predecessor that moved raw `Sad8x4` 6.49 → 2.18 ns between two runs of
one binary.

| shape | 16x16 | 16x8 | 8x16 | 8x8 | 8x4 | 4x8 | 4x4 |
|---|---|---|---|---|---|---|---|
| safe / raw | 1.41x | 1.51x | 2.38x | 2.30x | 3.89x | 4.89x | 4.66x |

Bar is ≤1.05x. Nothing is close, in any framing — D-perf-2's conclusion
re-derived from an instrument that can carry it.

Two things for whoever reopens this:

- **Per-row cost is the whole story and it falls off a cliff at W = 4.** 8x16
  costs more than 16x8 despite identical sample counts (12.6 vs 7.6 ns — 16 rows
  against 8), and 4x8 costs *twice* 8x8 for half the samples. Start at the narrow
  shapes; the wide ones are already within striking distance.
- **D-perf-2's slices-and-offsets lead remains untested.** The cheap probe —
  hoisting `PlaneCursor::new` out of the loop — is invalid, because
  `black_box(&cursor)` forces the cursor through memory and made the safe side
  *slower*; merely adding that third loop swung the 8x8 ratio 2.28x → 4.39x.
  Answering it needs a real slices-and-offsets kernel. Recorded rather than
  reported, because a contaminated column is how the last harness earned its
  caveat.

`encoder/sample.rs`'s SATD was parked by *projection* from the SAD numbers and
Phase 4a did not give it its own measurement either — the SAD verdict is a
strictly-easier lower bound (SATD is SAD plus a Hadamard butterfly), so the park
holds a fortiori, but the brief's ask for a real SATD measurement is **still
outstanding** and should be stated as such rather than treated as discharged.

**The safety argument for re-landing is now stronger than when the brief made
it.** F14 — production UB, found this session — is in `sad_common.rs`, the fourth
latent-UB finding in that one parked family (F10 three times, F14 once). It is
the family that has spent longest uninstalled with raw bodies live, and that is
not a coincidence.

---

## Phase 3 — the bitstream layer (2026-08-10 →)

### T3.1a — the decoder read side's bodies become `BsCursor` (shim at the raw signature)

Instrument: `perfpair.py`, A = `p4a_exit` (`25a8e287`), B = `t31a` (`d7bc0ac3` plus the
uncommitted seam), `FFMPEG` set. Session floor from `null t31a`, 1 pair: **decode
±0.2%** (median +0.04%, min -0.06%, max +0.18%), encode median +0.13% with one -6.85%
outlier, so the encode floor is the usual ≈±3%.

| | 1 pair | **3 pairs (medians)** |
|---|---|---|
| decode Constrained Baseline (CAVLC) | +1.41% | **+0.77%** |
| decode Main (CABAC) | -0.02% | **-0.26%** |
| decode High (CABAC, 8×8) | +0.24% | **+0.45%** |
| decode median | +0.24% | **+0.45%** |
| encoder median, 28 rows | -0.34% | **+0.00%** |

Read it as **+0.45% decode, encoder untouched**, and the cost sitting where the work
is: Constrained Baseline is the CAVLC stream, so it is the one that pulls every syntax
element through `BsGetBits`/`BsGetUe`, and it is the only row above the floor. The two
CABAC rows read far fewer elements through this family — their bit-level work is in
`cabac_decoder.rs`, which T3.2 converts.

The cost is the shim, not the cursor: every call now loads five fields out of
`SBitStringAux`, runs the safe body on a `BsCursor`, and stores three back, where the
raw version mutated the struct in place. **T3.1b deletes exactly that** by moving the
cursor into the structs that own the buffers, so this is a ledger row with a named
deletion point one seam away, not a carried deficit. No `#[inline]` change was needed:
the widths at the call sites are still literals, which is the property Phase 4a's
finding says to protect.

Cumulative decode after T3.1a: ≈ **+18.3 / +9.8 / +10.1%** (CB / Main / High) — the
tripwire is +25% median cumulative, so the phase still has ~7 points of headroom on CB.

### T3.1b — the ownership move: the shim above is deleted, and its cost comes back

Instrument: `perfpair.py`, A = `ctrl` (`1bf5a235`, the marshalling still in), B = `t31b`
(`773a91ac`), `FFMPEG` set. Session floor from `null t31b`, 3 pairs: **decode ±0.4%**
(median +0.08%, min −0.39%, max +0.26%); encode median +0.00% over 28 rows, min −2.76%,
max +1.49%, i.e. the usual ≈±3%.

| | pair set 1 | pair set 2 |
|---|---|---|
| decode Constrained Baseline (CAVLC) | **−0.93%** | **−1.07%** |
| decode Main (CABAC) | +0.23% | −0.24% |
| decode High (CABAC, 8×8) | +0.21% | −0.17% |
| decode median | +0.21% | **−0.24%** |
| encoder median, 28 rows | — | **+0.34%** |

**Read the CB row, not the median.** Two independent 3-pair sets put it at ≈−1%, same
sign, consistent magnitude, two to three times the floor — the shim field-marshalling
T3.1a predicted would return. The two CABAC rows sit inside the floor and change sign
between sets, which is what no signal looks like; their bit work is in
`cabac_decoder.rs` and belongs to T3.2. The encoder is a wash because this seam changes
zero bytes of `src/encoder/`.

**The T3.1a ledger row is closed, not carried.** Cumulative decode on the CAVLC row
across both halves of T3.1: +0.77% then ≈−1.0%, i.e. net slightly negative, so
cumulative decode returns to about its Phase-4a-exit level ≈ **+17.8 / +9.6 / +10.1%**.
The phase's ~7-point CB allowance is intact going into T3.2, which is the seam that will
actually spend it (`DecodeBinCabac` was 4a's largest single decode consumer at 544
self-samples).

### T3.2 — the CABAC engine's pointer triple becomes `pos` (2026-08-10, session C)

Instrument: `perfpair.py`, A = `t32-control` (`eae61b94`, the F17-fixed gate, engine
raw), B = `t32-head` (`00c6cf9f`), `FFMPEG` set. Session floor from `null t32-head`,
3 pairs: **decode ≈±2%** (median −0.27%, min −2.05% — the Main row moved 2% between
runs of one binary; the machine had been running batteries all day), encode median
−0.13%, min −3.46%, max +5.80% — a materially wider floor than session B's ±0.4%,
which sets what "signal" means below.

| | 3-pair medians |
|---|---|
| decode Constrained Baseline (CAVLC) | +0.19% |
| decode Main (CABAC, B-frames) | **+0.76%** |
| decode High (CABAC, 8×8) | **+0.27%** |
| decode median | +0.27% |
| encoder median, 28 rows | +0.00% (min −0.92%, max +2.94%) |

**This time the CABAC rows are the signal and they are inside the floor** — the exact
band the phase brief's §4 predicted for a conversion whose literal bit-counts stay
literal ("flat-to-win"). CB is the cross-check (its reads are CAVLC's) and is flat.
The encoder is the required wash: the seam changes zero bytes of `src/encoder/`.

The number to keep is not the bench delta but the **disassembly diff that preceded
it** (S1 step 1/3, in the log entry): the first converted shape *failed* that
comparison — `Read32BitsCabac` stopped inlining (a stack frame per bin) and the
ladder's `_` arm re-checked the length (three `panic_bounds_check` paths) — and was
restructured (`#[inline(always)]` + a `first_chunk::<N>()` chain, common arm first)
before any bench ran. Final shape: refill fully inlined, zero buffer bounds checks,
the range/offset `ldp` preserved, 122 vs ~121 instructions. Both defects passed every
test; only the disassembly saw them.

**No ledger row is opened.** Cumulative decode stays ≈ **+17.8 / +9.6 / +10.1%**
(tripwire arithmetic: +0.2/+0.8/+0.3 on top changes nothing at the 0.1 level against
a ±2% floor). The CB allowance is still intact; what is left of the phase's decode
work is T3.3's ownership seam, which touches no per-bin path.

---

### T3.4 — the encoder write side (2026-08-11, session E)

Instrument: `perfpair.py`, `FFMPEG` set. Session floor from `null face1`, 3 pairs:
encoder median **+0.00%**, min −1.69%, max +1.58% — one of the tightest floors this
project has measured, which is what makes the flat result below worth stating.
`Spatial Ramps` moved −53% in that null and +111% in the run; EXCLUDED per S2 both
times, and recorded because the rule says to.

| pair | 3-pair medians |
|---|---|
| control `b308f7d5` → face 1 (the F2 dedupe) | encoder **+0.00%** (−1.43% … +1.47%), decode **+0.00%** |
| face 1 → `fb4e7c29` (the whole conversion) | encoder **+0.00%** (−1.02% … +1.47%), decode **+0.00%** (−0.33% … +0.06%) |

Every row inside the floor, on both benches, for both halves of the seam. **No ledger
row is opened**; cumulative stays ≈ +8.9% encoder and ≈ +17.8 / +10.1 / +9.6% decode.
The decode figure is the required wash — the seam changes no decoder code — and the
face-1 pair is the required wash for a dedupe onto identically-inlined functions.

The number to keep here is again the disassembly, and this time it is the reassuring
direction. `WelsWriteVUI`, the representative literal-`n` writer, control vs HEAD:

| | control | HEAD |
|---|---|---|
| instructions | 765 | **720** |
| calls (`bl`) | 1 | 1 (the out-of-line cold bounds-failure path) |
| **constant-amount shifts** | **6** | **6** |
| variable-amount shifts | 43 | 40 |
| stores | 119 | **56** |
| branches | 77 | 122 |

Three readings, in order of how much they generalise:

1. **The literal-`n` rule held** (phase brief §4, 4a's finding). Constant-amount shifts
   are unchanged at 6 and there is no writer call in the body, so `write_bits` inlines
   whole and the literal widths still fold — nothing laundered a literal into a runtime
   argument. That was the specific regression the brief told this seam to watch for.
2. **The store collapse was free.** `WRITE_BE_32`'s four byte-stores became one 32-bit
   store because `copy_from_slice` on a fixed window compiles to the wide store the
   punned access existed for — S8's catalogued result, arriving without being asked
   for, and the reason the instruction count *fell* across a conversion that adds
   bounds checks.
3. **The bounds checks are the branch growth (77 → 122) and they cost nothing
   measurable.** They are out-of-line — a single cold `bl` at the tail serves all of
   them — so the hot path is straight-line. Worth recording because the phase's
   standing worry runs the other way: this is the seam that *adds* checking to the
   encoder's innermost writer, and it came out smaller and no slower.

---

## Phase 3 exit — the cumulative measurement, and a measurement lesson

**Commits.** Phase entry `5e5c9196` (session A's start) → exit `e3fc68e8`. Note the
session-F brief named `b308f7d5` as "the phase-entry baseline"; **it is not** — it is
session *E*'s start, after T3.0–T3.3 had already landed. Both spans are reported below
because the second isolates the three encoder-side seams.

**This session's floor (S2, null run — the same binary in both slots, 3 pairs):**

| bench | median | band | rows over +5% |
|---|---|---|---|
| decode | +0.04% | −0.33% … +0.40% | 0 |
| encode | +0.01% | −1.53% … **+22.57%** | **1** |

That encode row at +22.57% is the same binary against itself. **The machine was noisy
today**, and the rest of this section has to be read against that fact rather than
around it.

### The numbers, at both pair counts

**Whole phase, `5e5c9196` → `e3fc68e8`:**

| bench | 3 pairs | 5 pairs |
|---|---|---|
| decode median | **+0.68%** | **−1.93%** |
| decode rows | CB −0.47 / Main +0.68 / High +0.70 | CB −1.93 / Main −2.40 / High +0.46 |
| encode median | **+0.00%** (−1.38 … +1.45) | **+0.79%** (−1.20 … +2.94) |

**The three encoder-side seams only (T3.4+T3.5+T3.6), `b308f7d5` → `e3fc68e8`:**

| bench | 3 pairs | 5 pairs |
|---|---|---|
| decode median | **+1.21%** | **+0.16%** |
| decode rows | CB +0.11 / Main +1.36 / High +1.21 | CB +0.54 / Main **−0.03** / High +0.16 |
| encode median | **+0.00%** (−2.25 … +1.94) | **+0.00%** (−0.91 … +1.45) |

### What this says, and what it does not

**The lesson is the disagreement, and it is worth more than any single row here.**
Going from 3 pairs to 5 pairs moved whole-phase decode from **+0.68% to −1.93%** — a
2.6-point swing that *changed the sign* — and moved the encoder-only decode median from
+1.21% to +0.16%, with the Main row going +1.36% → −0.03%. **Three interleaved pairs
did not converge on this machine today.** The protocol's 3-pair default is a floor, not
a guarantee; when a median lands outside the null band, the first move is more pairs,
not a diagnosis. That cost this exit two extra runs and saved it a fabricated
regression: the +1.21% decode figure, taken at face value, would have been written up
as a real cost of encoder-side seams that touch no decoder code.

**No ledger row opens.** Every median is far inside the ≤5%-per-phase gate and nowhere
near the +25% median cumulative tripwire. Two measurements of the same span at
different pair counts disagree in sign, which is direct evidence that the true effect
is smaller than the measurement error — the D-perf-4 disposition for that is
diagnostic-only, and this paragraph is the diagnosis.

**The two defensible statements:**

1. **The encoder is unmoved.** `+0.00%` at both pair counts on the encoder-only span,
   and the encoder-side seams are the only ones that could have touched it. Cumulative
   encoder stays ≈ **+8.9%** vs Phase 2's start, as it has since Phase 4a's checkpoint.
2. **Decode did not regress, and probably improved.** The best-converged whole-phase
   number is −1.93% (5 pairs), consistent with T3.1b's recorded expectation that the
   decoder seams would return about a point. Cumulative decode is therefore **no worse
   than ≈ +17.8 / +10.1 / +9.6%** (CB / Main / High) and likely a point or two better;
   the phase's ~7-point CB headroom under the tripwire is intact and is Phase 5's to
   spend.

`Spatial Ramps` printed −42.17% / −43.04% and is **EXCLUDED** per S2 — the same rows
the null run has shown at ±38%, −11% and +226% across runs of one binary.

### What Phase 3 cost, structurally

Nothing measurable, on a phase that added bounds checking to *both* bitstream hot
paths and moved three encoder allocations from `CMemoryAlign` to `Vec`. The T3.4
disassembly explains the writer half (bounds checks are out-of-line; `WRITE_BE_32`'s
four byte-stores became one 32-bit store via `copy_from_slice`, so the seam came out
*smaller*), and T3.6's allocations happen once per encoder init rather than per frame.

---

## Phase 4b — configuration dispatch (2026-08-11 →)

### T4b.1 — the entropy dispatch becomes one enum

Instrument: `perfpair.py`, `FFMPEG` set. Session floor from `null t4b1_ctl`, 3 pairs:
encoder median **+0.00%** (−1.45% … +1.45%), decode median **−0.16%**
(−0.20% … +0.05%). Both are tight floors — tighter than the Phase 3 exit session's
encode band, which ran to +22.57% and is what earned S2b.

| pair (1 pair, per §1.3 of the brief) | median | band |
|---|---|---|
| control `6e15c907` → `08b7c29d` **encode** | **+0.05%** | −5.54% … +2.99% |
| control `6e15c907` → `08b7c29d` **decode** | **+0.08%** | −0.13% … +0.51% |

**No ledger row opens**, and the result is the *predicted* one: the brief's §1.2 said
in advance not to expect the enum to buy speed, because 4a's finding is that direct
dispatch recovers per-call scaffolding only where the caller supplies constant
dimensions — and these are per-macroblock calls with a runtime-selected arm. What the
seam buys is a deleted `Option`, a deleted thunk, a deleted parameter and a signature
the compiler can see through. It bought exactly that, and no time.

Two notes on reading the table. The encode band is wider than the null's at one pair
(−5.54% … +2.99% against ±1.45%), which is what a single pair looks like rather than
evidence of anything — S2b's remedy is more pairs, and the phase-exit protocol runs
them; the median is the statistic. The decode figure is the required wash: the seam
changes no decoder code at all, so +0.08% is a floor reading and nothing else.

`Spatial Ramps` printed −8.00% / −9.04% in the null and is **EXCLUDED** per S2.

### T4b.1b — the rate-control table becomes one mode, and S2b pays again

Same session, same floor (the null above).

| pair | median | band |
|---|---|---|
| `08b7c29d` → `3e583b9a` **encode**, 1 pair | +0.10% | −2.09% … **+22.91%** |
| `08b7c29d` → `3e583b9a` **encode**, 3 pairs | **−0.15%** | −0.93% … +1.32% |
| `08b7c29d` → `3e583b9a` **decode**, 3 pairs | **−0.06%** | −0.38% … +0.04% |

**The row to look at is `640x480 (VGA Mandelbrot) [4t]`: +22.91% at one pair,
−0.49% at three.** A 23-point swing, on a row whose 1-thread twin read +0.08% in
the same run, from nothing but a higher pair count. This is S2b's second payment
in two phases, and it is a sharper instance than the one that earned the rule:
at Phase 3's exit the swing was 2.6 points and changed a sign; here it is an
order of magnitude larger and would have been written up as a 23% regression on a
seam that deletes nine function pointers and adds no work to any inner loop.

The whole-run medians (+0.10% → −0.15%) barely moved, which is the practical
reading: **the median was right at one pair and the row was not.** A per-row
maximum is a single sample no matter how many rows are in the table, and the
brief's "one interleaved pair per bench per seam" buys a median, not a row.

**No ledger row opens** for either seam. Cumulative is unchanged at ≈ +8.9%
encoder and ≈ +17.8 / +10.1 / +9.6% decode.

### Session B — T4b.2a, T4b.2b, T4b.3a (three seams, one floor)

Session floor from `null t4b2a_ctl`, 3 pairs: decode median **-0.50%**
(-1.02% … +1.68%), encode median **+0.22%** (-2.68% … +2.75%). The encode band is
wider than session A's ±1.45% because a `cargo build` overlapped the null's last two
pairs; the interleave puts that load on both slots, so it widens the band rather than
biasing the median, and a wider floor is the conservative error.

**Every seam ran 3 pairs from the start**, not one. That is S2b applied in advance
rather than after a scare: T4b.1b's `640x480 [4t]` row read +22.91% at one pair and
-0.49% at three, and the cost of three pairs is minutes.

| seam | pair | decode median | encode median |
|---|---|---|---|
| T4b.2a (paraset strategy) | `t4b2a_ctl` → `t4b2a_head` | **+0.12%** (+0.05% … +0.31%) | **+0.47%** (-1.45% … +3.07%) |
| T4b.2b (reference strategy) | `t4b2a_head` → `t4b2b_head` | **+0.38%** (+0.15% … +0.64%) | **-0.04%** (-1.81% … +1.45%) |
| T4b.3a (intra-pred constraint) | `t4b2b_head` → `t4b3a_head` | **+0.17%** (-0.27% … +0.19%) | **+0.00%** (-1.06% … +2.97%) |

**No ledger row opens for any of the three.** Cumulative unchanged at ≈ +8.9% encoder
and ≈ +17.8 / +10.1 / +9.6% decode.

Two readings worth keeping:

1. **Each seam's "own" bench is the flat one, and its other bench is the wash.**
   T4b.2a and T4b.2b are encoder-only and read +0.47% / -0.04% there; T4b.3a is
   decoder-only and reads +0.17% there. The off-side figures are floor samples and
   nothing else — worth stating because a reader scanning the decode column will see
   +0.12 / +0.38 / +0.17 and could mistake a trend for a cost. Two of those three
   numbers come from code that was not touched.
2. **Three de-virtualizations in a row bought no measurable time, as predicted.** 4a's
   finding says direct dispatch recovers per-call scaffolding only where the caller
   supplies constant dimensions. Every arm here is runtime-selected — from a parameter
   set, a usage type, a PPS flag — so there was nothing to recover, and the session
   spent its measurement budget confirming that rather than discovering it. What the
   three seams bought is in the ratchet: `raw_ptr` -164, `unsafe_fn` -39,
   **`transmute` 23 → 5**.

---

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

### Session C — T4b.3b, T4b.3c, and the whole-phase measurement

Session floor from `null t4b3a_ctl`, 3 pairs: decode median **-0.24%**
(-0.47% … +0.23%), encode median **+0.21%** (-0.77% … +1.45%).

**Read the floor first this session: it is not centred on zero.** The decode null came
back at -0.24%, so a seam reading -0.5% is reading the floor, not an effect. This is
the same instrument-before-result discipline S2b asks for, applied to the *sign* rather
than the magnitude.

| pair | decode median | encode median |
|---|---|---|
| T4b.3b (expand family) | **-0.64%** (-1.13% … +0.13%) | **+0.00%** (-1.78% … +1.45%) |
| T4b.3c (`sBlockFunc`) | **-0.47%** (-0.53% … +0.03%) | **+0.32%** (-0.34% … +1.80%) |
| **whole phase**, `6e15c907` → `f2e3c5af` | **-0.36%** (-0.84% … -0.31%) | **+0.08%** (-2.53% … +1.14%) |

**No ledger row opens.** Cumulative unchanged at ≈ +8.9% encoder and
≈ +17.8 / +10.1 / +9.6% decode.

Three readings worth keeping.

1. **All four decode medians are negative and cluster inside 0.4 points of each other
   — including the null.** -0.24 (floor), -0.64, -0.47, -0.36. The honest statement is
   that this session's decode instrument sat slightly below zero all evening and every
   seam sampled it. T4b.3b's -0.64% is the only median outside the floor's band, by
   0.17 points, in the *favourable* direction, on a three-row median. Per S2b that
   would earn more pairs before it earned a mechanism — but there is no mechanism to
   claim: no ledger row opens for a gain, and D-perf-4's tripwire is a regression
   tripwire. **Recorded as flat.**
2. **Phase 4b measured flat end to end.** Five seams across three sessions, every one
   inside the floor, and a whole-phase number (`6e15c907` → `f2e3c5af`) of +0.08%
   encode / -0.36% decode. This is exactly what Phase 4a's finding predicts and the
   phase brief expected in advance: **direct dispatch recovers per-call scaffolding
   only where the caller supplies constant dimensions**, and every arm this phase
   removed was runtime-selected — a parameter set, a usage type, a PPS flag, a CPU flag
   that was never read. There was nothing to recover, and the phase spent its
   measurement budget confirming that rather than discovering it.
3. **So the phase's return is entirely in the ratchet**, and that is a legitimate
   result rather than a disappointment: `raw_ptr` 5001 → 4815, `unsafe_fn` 1286 → 1250,
   `transmute` 23 → **4, all prose, zero calls**, two vtables and 25 thunks deleted, two
   duplicate families collapsed and two findings fixed. Phase 5 is where the decode
   numbers are supposed to move, because 5.x is what makes `BaseMC`'s dimensions static.

---

## Phase 5 — the decoder structural rewrite (2026-08-12 →)

### 5.2's flip, and D-perf-5's retrofit: what a per-macroblock bounds check actually costs

Three spans, all 7 interleaved pairs, all on one machine, `perfpair.py` (S1/S2). The
session-I null floor, measured the same day: decode **±0.6%** (3 rows, median +0.19%,
−0.21%…+0.60%); encode 28 rows, median −1.34%, −12.07%…+8.14%.

| span | CB (CAVLC) | Main (CABAC) | High (8x8) | decode median |
|---|---|---|---|---|
| the flip, session H (`3c4c6f4e` → `1438d762`) | +2.05% | +0.49% | +1.32% | **+1.32%** |
| **the same span, re-run session I** | **+1.18%** | **+0.65%** | **+0.33%** | **+0.65%** |
| the window retrofit (`c51f802d` → `17e5b7ba`) | −0.08% | +0.22% | +0.14% | **+0.14%** |

Two results, and the second is the one to carry.

**1. The window idiom works mechanically and buys nothing.** Opcode histograms over the
six CAVLC per-macroblock functions — an `MbArray` index is exactly `ldr` (the length),
`cmp`, `b.ls`, so `b.ls` counts them:

| | instructions | `b.ls` |
|---|---|---|
| pre-flip | 2937 | **2** |
| post-flip | 3199 | **78** |
| post-retrofit | 3331 | **38** |

The flip added **76 bounds checks per macroblock** at three instructions each — ~228 of
the 262 instructions it added, so session H's attribution of the cost to bounds checking
was right. The retrofit removed **40 of the 76**, exactly as designed, and the bench
cannot see it: every row is inside the null band. The checks are perfectly predicted and
the length they load sits in L1 beside the pointer.

The hoist also **costs** code size. `WelsDecodeMbCavlcResidual` went 703 → **961**
instructions, because lifting the QP load out of the residual loop made the body simple
enough for LLVM to unroll far harder (`WelsResidualBlockCavlc` call sites 16 → 31). Net
across the six functions: −40 branches, +132 instructions.

`/usr/bin/sample` over the CB portion, self time, the harness's SHA-1 excluded, gives the
magnitude directly: `DecodeBinCabac` 4.9%, `deblock_luma_lt4` 4.7%, `McChroma_c` 3.4%,
`deblock_chroma_lt4` 3.0%, `ParseSignificantCoeffCabac` 2.4%, `BaseMC` 2.4%, and the
highest-placed function this retrofit touched is `WelsDecodeMbCavlcPSlice` at **1.2%**.
**A 2% whole-stream recovery was never available from this code.**

**2. One span, two seven-pair readings, a factor of two apart.** The flip's own span
re-ran today from the same two stashed binaries at +0.65% median where session H recorded
+1.32%, and the row carrying the median swapped ends. The cost is real — every row is
positive in both readings — and its magnitude is not stable across days at seven pairs.
S2b says three pairs is a floor and not a guarantee of convergence; **seven is not one
either**, and a decision resting on a single span number wants a second reading on a
different day rather than more pairs on the same one.

**What this does not say.** The eleven families measured here are one scalar per
macroblock, so a window only ever amortises repeat visits to the same entry. The eleven
still to flip are not that shape — `pNzc` is `[i8; 24]`, `pMv`/`pMvd` are
`[[i16; 2]; 16]`, `pScaledTCoeff` is `[i16; 384]` — and are indexed *inside* the record
in inner loops, where the element re-type makes the inner index const-bounded and
check-free. **That is a different mechanism and it is untested. This null result does not
transfer to them in either direction.**

### T5.J1 and T5.J3 — the first hot family, and it is free (session J, 2026-08-12)

Session-J null floor, measured at open: decode **+0.13%…+0.45%** (3 rows, median +0.35%)
— tighter than session I's ±0.6%; encode 28 rows, median +0.00%, −2.45%…+1.42%.

| span | pairs | CB (CAVLC) | Main (CABAC) | High (8x8) | decode median | encode median |
|---|---|---|---|---|---|---|
| **T5.J3 — `pRefIndex` flips** (`d8becdf0` → `e207fdd9`) | **7** | +0.24% | +0.03% | −0.03% | **+0.03%** | +0.00% |
| T5.J1+J2 — F35's unaligned spellings (`60d1528b` → `d8becdf0`) | 3 | +0.77% | −0.03% | −0.23% | **−0.03%** | +0.10% |

**`pRefIndex` is the first of the hot eleven and it costs nothing measurable.** Every row
is inside the session's null band and **two of the three are below its floor**. This is
the family D-perf-5 named as one of the untested ones — `[i8; 16]`, indexed *inside* the
record — and it is tied for hottest by profile: 117 of 3769 self samples, 3.1%
(`PredMvBDirectSpatial` 56, `DeblockingBsMarginalMBAvcbase` 40, `UpdateP16x16MotionInfo`
15). **No ledger row opens.**

**Why this is plausible rather than surprising**, stated so the next family is judged
against a mechanism and not against a hope: session H's eleven were scalars, so every
access was a *separate* bounds check on a separate array — 76 per macroblock. `pRefIndex`
is one check per macroblock *record*, after which the sixteen in-record indices are
const-bounded against a `[i8; 16]` and fold. The families still to flip that share that
shape are `pMv`, `pMvd`, `pNzc`, `pDirect`, `pScaledTCoeff` and `pIntraPredMode`; the ones
that do not are `pMbType` and `pSliceIdc`, which are scalars and should behave like
session H's.

**This reading owes a day-two confirmation and has not had one.** S2b as extended by
session I: two seven-pair readings of one span disagreed by a factor of two on different
days. A decision does rest on this number — whether the remaining nine keep flipping — so
per D-perf-5's direction the confirmation is the **first item of the next session**, from
the same two stashed binaries (`.perfpair/j_face1`, `.perfpair/j_j3`).

**Cumulative CB after this family: ≈ +19.2…+20.1%**, the range being session H's and
session I's readings of the same earlier span, against the ≈**+23%** stop-line. Nothing is
near it.

**F35's span is a wash and no row opens for it either.** It is a soundness fix that lands
regardless, but it rewrote 78 accesses in `mv_pred.rs`'s MV-cache helpers, so it was
measured rather than waived: decode median −0.03% at 3 pairs. The CB row reads +0.77%,
above the null's +0.45% ceiling and the only row of either span that is; the median is the
verdict and S2b's escalation applies to medians outside the band, which this is not.
`read_unaligned`/`write_unaligned` compiles to the same instruction as an aligned access
on both targets this project builds, which is what the number says and also why no byte
gate could ever have caught the defect.

### Session K — three more families, and the day-two confirmation that turned into a result about the instrument (2026-08-12)

**Read this section for the second half.** The family rows are unremarkable and that is
their point; the confirmation is the finding.

Session-K null floors, both measured this session on the same machine, same harness,
same binary in both slots:

| null | decode band | decode median | encode (28 rows) |
|---|---|---|---|
| **3 pairs** | −0.16% … −0.02% | −0.04% | median +0.00%, −1.45%…+3.09% |
| **7 pairs**, matched to the verdicts | **−0.22% … +0.25%** | +0.04% | median +0.02%, −1.43%…+3.36% |

#### 1. The day-two confirmation of T5.J3 — three readings of one span

| reading | CB (CAVLC) | Main (CABAC) | High (8x8) | decode median | encode median |
|---|---|---|---|---|---|
| day one (session J, 7 pairs) | +0.24% | +0.03% | −0.03% | **+0.03%** | +0.00% |
| **day two #1** (7 pairs) | +0.27% | +0.32% | −0.11% | **+0.27%** | −0.05% |
| **day two #2** (7 pairs) | −0.45% | −0.05% | −0.13% | **−0.13%** | +0.01% |

**The two same-day readings disagree in sign, and so does the CB row across all three**
(+0.24 / +0.27 / −0.45). S2b's own criterion for "the effect is below the measurement
error" is met, twice over. The mechanism `perf_baseline`'s J section claims — one bounds
check per macroblock *record*, in-record indices const-bounded — stands: nothing this
instrument can see is being spent.

**What made this worth the extra two runs is which way the answer flipped.** Judged
against the **3-pair** null, day two #1's +0.27% sits *outside* the band and the session's
brief would have stopped on it, escalating a non-result. Judged against the **7-pair**
null run half an hour later on the same machine, it sits *inside*. A band is a
min-to-max over three bench rows; it is a sample, and a 3-pair sample is not a floor for a
7-pair verdict. That is now written into S2b.

**And the larger conclusion, which is what the next session should act on.** Three
readings spanning 0.4 points, three session nulls spanning 0.6 points between them, and
an effect somewhere near zero: **the quantity the plan needs is the same size as the
noise.** Ten families over ≈3 points of headroom to the ≈+23% stop-line is ≈0.3% CB per
family — precisely the resolution limit demonstrated here. **No reachable pair count
resolves a per-family cost.** The cumulative figure is ~20% and this harness resolves it
trivially, so the instrument to use from here is **one 7-pair span from the pre-flip base
(`seat_head` 3c4c6f4e / `flip_head` 1438d762, both stashed) to HEAD** — the number the
stop-line actually gates on. Session I already found the per-family sum high by a factor
of two, which is the same defect seen from the other end.

#### 2. The three families

| span | pairs | CB (CAVLC) | Main (CABAC) | High (8x8) | decode median | encode median |
|---|---|---|---|---|---|---|
| **T5.K1 — `pMv`**, the hottest family (3.9%) (`e207fdd9` → `31826d05`) | 7 | +0.04% | −0.13% | −0.20% | **−0.13%** | +0.09% |
| **T5.K2+K3 — `pMbType` + `pSliceIdc`**, both scalars (`31826d05` → `135ebcb7`) | 7 | +0.04% | −0.65% | +0.24% | **+0.04%** | +0.00% |

Every row of both spans is inside the 7-pair null band except Main's −0.65%, which is
below its floor — noise in the favourable direction. **No ledger row opens for any of the
three families.**

`pMv` is the single hottest family in the whole flip by profile — 102 of 2610 non-SHA-1
self samples, 3.9%, in `DeblockingBsMarginalMBAvcbase`, `PredMvBDirectSpatial` and
`PredPSkipMvFromNeighbor` — and it is free, which is the third consecutive record-shaped
family (`[T; K]` indexed inside the record) to come in at nothing. `pMbType` and
`pSliceIdc` are **scalars**, the shape session H measured at +0.65…+1.32% for eleven of
them, and two of them together also read nothing.

**The two spans are measured per commit and per cluster respectively, and the cluster is
deliberate**: after §1, splitting a ≈0.1% effect into two ≈0.05% halves is measuring
below the floor twice instead of once. Both commits exist, so per-family spans can be
rebuilt from the refs at any time if a reason appears; the measurement is not perishable.

**Cumulative CB after fifteen families: ≈ +19.2…+20.1%**, unmoved — every span since
session H has read inside a null band, which is exactly the state that makes §1's point.
*(Superseded by session L's direct reading below: **≈ +20.2%**, measured as one span
rather than summed.)*

### Session L — the cumulative span, measured instead of summed (2026-08-12)

Session-L null floor, 7 pairs, matched to the verdicts' pair count (S2b):

| bench | median | band | rows over +5% |
|---|---|---|---|
| decode (3 rows) | **+0.22%** | +0.15% … +0.31% | 0 |
| encode (28 rows) | **+0.00%** | −2.80% … +1.94% | 0 |

#### The two spans

| span | pairs | CB (CAVLC) | Main (CABAC) | High (8x8) | decode median | encode median |
|---|---|---|---|---|---|---|
| **the whole 5.2 flip** — pre-flip seat → HEAD (`3c4c6f4e` → `a4670ef6`) | 7 | **+2.36%** | +0.61% | +0.27% | **+0.61%** | +0.18% |
| sessions I+J+K only — post-session-H → HEAD (`1438d762` → `a4670ef6`) | 7 | +0.46% | −0.30% | +0.08% | **+0.08%** | +0.00% |

**The whole-flip CB row is the first Phase 5 reading that is unambiguously above the
floor**: +2.36% against a null band 0.16 points wide, seven times the band's width. That
is the point of measuring the aggregate — the same harness that cannot resolve 0.3% per
family resolves 2.4% across fifteen of them without difficulty.

**The two spans are consistent with the per-session record.** Subtracting them gives
session H's eleven families at **+1.90% CB**, between session H's own +2.05% (day one)
and session I's +1.18% (day two) readings of that exact span. Everything since session H
— the window retrofit, F35's spellings, and the four hot families — is **+0.46% CB**, and
its decode median (+0.08%) sits *below* the null's floor.

#### The cumulative position, and the correction it forces

**Cumulative CB ≈ +20.2%** — Phase 4a's exit position (+17.8%, unmoved by Phases 3 and
4b, both of which measured flat end to end) plus this directly-measured whole-flip span —
against the ≈**+23%** stop-line. **≈2.8 points of headroom for the remaining seven
families**, which is the position Face 2 started from; the same span re-measured after
all seven had flipped reads +2.93% and puts the cumulative at ≈ +20.7% (below).

**The summed estimate was not high; it was slightly low.** ≈+19.2…+20.1% summed against
≈+20.2% measured. Session I's "the sum is high by roughly a factor of two" was a
statement about *one span read twice on two days* (+2.05% → +1.18%), and it does not
generalise to the sum: read as one span, the flip's cost lands at the **top** of the
summed range, not at half of it. The instrument change stands on its own merits — one
reading above the floor beats fifteen below it — but the headroom it buys is what the
plan already assumed, not more.

**The whole-flip reading owes a day-two confirmation** (S2b) — the decision it carries
is how much of the ≈+23% stop-line 5.2's remaining work and 5.3–5.6 may spend — so the
confirmation is the next session's first item, run against the **final** span
(`.perfpair/seat_head` → `.perfpair/l_c2`, both stashed) rather than this morning's.

#### The last seven families, and the first cluster reading that is outside the floor

Measured after Face 2, same day, same null:

| span | pairs | CB (CAVLC) | Main | High | decode median | encode median |
|---|---|---|---|---|---|---|
| **T5.L1–L7 — the last seven families** (`a4670ef6` → `f63e8ef6`) | 7 | **+1.27%** | +0.41% | +0.52% | **+0.52%** | +0.00% |
| **the whole 5.2 flip, all 22 families** (`3c4c6f4e` → `f63e8ef6`) | 7 | **+2.93%** | +0.97% | +1.01% | **+1.01%** | +0.00% |

**Every row of the seven-family span is above the null band's ceiling** (+0.31%), and the
CB row is four times the band's width. This is the first family-cluster reading of the
whole flip that lands unambiguously outside the floor, and it is the same lesson Face 1
taught, running the other way: **the per-family readings were not merely noisy, they were
systematically under-reporting.** T5.J3 read +0.24% CB, T5.K1 +0.04%, T5.K2+K3 +0.04% —
each "free" — and seven families measured together read +1.27%. An effect at the
resolution limit does not average out to nothing; it hides, one family at a time.

**Cumulative CB is now ≈ +20.7%** — Phase 4a's +17.8% plus the whole-flip +2.93% —
against the ≈**+23%** stop-line. **≈2.3 points of headroom** for everything 5.2 has left
(the `DqLayerState` rename, 32 scratch-cache re-points, `pBitStringAux`) plus 5.3–5.6.

Two notes on the arithmetic, both of which matter more than the third decimal:

* **The spans are not perfectly additive at this resolution.** The flip through session K
  read +2.36% CB this morning and the seven families read +1.27% this evening, which
  would sum to +3.63%; the direct whole-flip span reads **+2.93%**. Half a point of slop
  across three 7-pair readings taken hours apart is the honest error bar on any
  *composed* figure — which is the argument for measuring the whole span directly rather
  than adding, and this section does both so the gap is visible instead of assumed.
* **The machine moved during the session and the protocol absorbed it.** `seat_head`'s CB
  column read 2.5460 ms this morning and 2.4920 ms this evening — 2% faster — while the
  span it carries barely moved. That is interleaved pairs (S1) doing exactly the job they
  exist for; an unpaired reading taken twelve hours apart would have reported the
  machine's afternoon instead of the port's.

Per family the flip costs **≈0.13% CB averaged over 22**, below the ≈0.3% the plan's
arithmetic assumed and above the "nothing" the per-family readings kept reporting.
**Encode is flat at +0.00% median on both spans**, as a decoder-only change should be.
**No ledger row opens**: the cost is not scaffolding that a later phase deletes — it is
the bounds check the safe container exists to have — so it belongs in the cumulative
position rather than in a deficit ledger that promises a recovery.

#### The absolute deficit against the C++ dylib, and why it is not this number

Read off the decode bench's own two columns, best of 3 interleaved passes per label,
both labels against the same dylib (so the C++ column is the control — it moved 0.2–0.4%
between labels, which bounds the drift):

| stream | `seat_head` Rust vs C++ | `l_head` (mid-session HEAD) Rust vs C++ |
|---|---|---|
| Constrained Baseline | +54.0% | **+56.8%** |
| Main | +2.5% | +3.1% |
| High | +3.2% | +4.2% |


**This is not the cumulative deficit and must not be quoted as one: the dylib dispatches
NEON.** The bench prints it — `C++ SIMD : ACTIVE (WelsCPUFeatureDetect = 0x000006)`, and
`0x4` is `WELS_CPU_NEON` — so the CB row compares scalar Rust against hand-written NEON
deblocking and MC, which is why it is 56% where the CABAC-bound rows are 3–4%. §7.4's
parenthetical that "the C++ dylib never dispatches SIMD" is stale for this build and is
corrected in place; the project's cumulative figures are chains of **Rust-vs-Rust** spans
anchored at Phase 0 and are unaffected. What the table does add is a check on direction:
the whole-flip span reads +1.5% CB here against +2.36% paired, same sign, same order of
magnitude, on an instrument that shares nothing with `perfpair` but the binaries.

### Session M — the day-two confirmation, and the cumulative position settles (2026-08-13)

Session-M null floor, 7 pairs, matched to the verdict's pair count (S2b):

| bench | median | band | rows over +5% |
|---|---|---|---|
| decode (3 rows) | **+0.03%** | +0.02% … +0.19% | 0 |
| encode (28 rows) | **+0.00%** | −1.47% … +0.75% | 0 |

#### The confirmation

Same two stashed binaries, same 7-pair protocol, a different day — the reading session L's
cumulative position and the whole phase's remaining headroom rest on:

| reading | day | CB (CAVLC) | Main | High | decode median | encode median |
|---|---|---|---|---|---|---|
| the whole 5.2 flip, all 22 families (`3c4c6f4e` → `f63e8ef6`) | L (08-12) | **+2.93%** | +0.97% | +1.01% | +1.01% | +0.00% |
| the same span, re-read | **M (08-13)** | **+2.57%** | +0.65% | +0.79% | +0.79% | +0.03% |

**It confirms on every axis S2b asks about.** Same sign on all three decode rows; CB within
**0.36 points** of day one against a criterion of ~1 point; every decode row above the null
band's ceiling (+0.19%) rather than merely the median; encode flat at +0.03% median, as a
decoder-only change should be. The two readings bracket the flip's cost at
**+2.6…+2.9% CB**.

**This is the first Phase 5 span to survive a day-two confirmation intact, and the contrast
with the per-family readings is the whole argument for aggregates.** T5.J3's one span, read
three times at 7 pairs, gave +0.03%, +0.27% and −0.13% — disagreeing in *sign*. The
22-family span, read twice on two days, gives +2.93% and +2.57% — agreeing in sign, in
magnitude, and row by row. Nothing about the harness changed between those two experiments
except the size of the thing being measured.

#### The cumulative position, settled

**Cumulative CB ≈ +20.4…+20.7%** — Phase 4a's exit position (+17.8%, unmoved by Phases 3
and 4b) plus the two readings of the whole-flip span — against the ≈**+23%** stop-line.
**≈2.3–2.6 points of headroom**, and the conservative end is the one to spend against: the
rest of 5.2 (`DqLayerState`, the 32 scratch-cache re-points, `pBitStringAux`) plus all of
5.3–5.6 live inside **≈2.3 points**.

The figure is no longer provisional. Session L measured it and this session confirmed it,
which is what S2b's day-two clause exists to produce; no later session needs to re-derive
it from per-family sums, and none should.

#### The session's own span — 5.2's tail and 5.3a, measured whole

Per Face 1's rule (cluster or whole-session span, never a per-face half — S2b, and
session L §2's correction), the four code faces are one span:

| span | pairs | CB (CAVLC) | Main | High | decode median | encode median |
|---|---|---|---|---|---|---|
| **T5.M1–T5.M4** (`f63e8ef6` → `16a6130c`) | 7 | **−0.77%** | −0.08% | −0.25% | **−0.25%** | +0.03% |
| session null (same binary both slots) | 7 | — | — | — | +0.02% … +0.19%, median **+0.03%** | +0.00% |

**Every decode row lands below the null band's floor, and CB by four times the band's
width.** This is the first Phase 5 span to land *negative* outside the floor, judged by
exactly the standard that made session L's +2.93% real.

Two things it is, and one it is not. It **is** a clean answer to the question the session
had to ask — the ≈2.3 points of headroom are untouched, so 5.2's tail and 5.3's first half
were free. It **is** mechanically plausible: T5.M2 turned 45 pointer parameters into
borrows, so 167 dereferences became indexed reads of arrays whose length is a constant;
T5.M3 deleted a pointer chase from every parsing function's prologue; T5.M4 deleted eight
duplicate function bodies. It is **not** a claim that the port got faster — one row of a
three-row bench, a 0.17-point null, and no second day. The honest statement is the one the
headroom rests on: **this session cost nothing**, and cumulative CB stays at
**≈ +20.4…+20.7%** against the ≈+23% stop-line.

### Session N — the `PicId` cluster, and the first Phase 5 session that is not free (2026-08-13)

Session-N null floor, 7 pairs, matched to the verdict's pair count (S2b):

| bench | median | band | rows over +5% |
|---|---|---|---|
| decode (3 rows) | **+0.03%** | −0.14% … +0.34% | 0 |
| encode (28 rows) | **+0.00%** | −1.45% … +1.47% | 0 |

#### The session's span, measured whole

| span | pairs | CB (CAVLC) | Main | High | decode median | encode median |
|---|---|---|---|---|---|---|
| **T5.N1–T5.N5** (`16a6130c` → `d0b7f399`) | 7 | **+1.24%** | **+1.90%** | **+0.75%** | **+1.24%** | −0.04% |
| session null (same binary both slots) | 7 | — | — | — | +0.03%, band −0.14% … +0.34% | +0.00% |

**Every decode row is above the null band's ceiling**, the lowest of them (+0.75%) by more
than twice, judged by exactly the standard that made session L's +2.93% real and session
M's −0.77% real. Encode is flat, as a decoder-only session should be. **This session cost
something**, and it is the first Phase 5 session since the flip itself of which that is
true.

#### What it does to the position

Cumulative CB ≈ **+21.6…+21.9%** against the ≈**+23%** stop-line — **≈1.1…1.4 points of
headroom** where session M left ≈2.3, for all of 5.5 and 5.6, which are the phase's two
largest remaining steps. The conservative end is the one to plan against.

**This reading owes a day-two confirmation and it is session O's first item** (S2b: a
reading a decision rests on gets a second day, not more pairs on the same one). It is the
same debt session L owed session M, and the reason is the same — the number changes what
the remaining phase can afford. Three stashed binaries are in `.perfpair/`: `n_base`
(`16a6130c`), `n_mid` (`9da4bede`) and `n_head` (`d0b7f399`).

#### The bisect — D-perf-4's one short look, spent on S5 rather than on a theory

+1.24% CB is over half the headroom session M left, so the cost is worth locating before
it is ledgered. S5's move is to split the span by commit before optimising anything, and
one extra build plus one 7-pair run does it. The mid-point is `9da4bede`, the end of Face 1:

| half | CB (CAVLC) | Main | High | decode median | encode median |
|---|---|---|---|---|---|
| **Face 1** — the pool and picture identity (`16a6130c` → `9da4bede`) | **−0.72%** | +0.11% | +0.13% | **+0.11%** | +0.08% |
| **Face 3** — the deblocking driver (`9da4bede` → `d0b7f399`) | **+1.17%** | **+2.25%** | **+2.06%** | **+2.06%** | +0.00% |
| session null | — | — | — | +0.03%, band −0.14% … +0.34% | +0.00% |

**Face 1 is free and Face 3 owns all of it.** Face 1's two CABAC rows sit inside the null
band and its CB row below the floor; every Face 3 row clears the ceiling by 3–6×. (The
halves do not sum to the whole — CB −0.72 + 1.17 = +0.45 against the span's +1.24, High
+2.19 against +0.75. Two 7-pair halves and one 7-pair whole disagree by about a point,
which is session K's result restated: the *yardstick* moves. The direction is not in doubt;
the magnitude of either half is worth less than the whole span's number, and the whole
span's is the one to carry.)

#### Which half of Face 3, and the hypothesis — **unverified, and named as such**

Face 3 is two conversions: T5.N3 (the plane mirror dies, so three plane pointers and two
strides are derived per macroblock instead of read from the filter struct) and T5.N4 (the
reference lists become `Option<PicId>`, so boundary strength compares ids).

**The profile asymmetry points at T5.N4.** Constrained Baseline costs +1.17% while Main and
High cost +2.25% and +2.06% — roughly double. T5.N3's derivation is slice-type-agnostic:
every macroblock of every profile deblocks, so it should cost the same everywhere. T5.N4's
extra work is not: the two-list `iRefs` fill and the `IN_SMB_EDGE_MV` / `ON_MB_BS` paths
that compare six ids per edge run on **B slices**, which is exactly what separates Main and
High from CB.

If that is right, the mechanism is the representation and the fix is structural rather than
an optimisation box: `Id` is `{ index: u32 }` in release, so `Option<Id>` is eight bytes
with a separate discriminant and `==` compares two fields where the pointer form compared
one. A niche — `index: NonZeroU32` holding `slot + 1` — makes `Option<Id>` one word and its
equality one compare, in `safe/pool.rs` and nowhere else. §7.4's "fast-by-construction"
clause covers picking the right representation; this is not a box.

**None of that is measured.** It is a hypothesis with one supporting observation, written
down so session O can settle it with one build rather than rediscover it — and S1's rule
stands: disassemble before believing it. The three-way experiment session O owes is: the
day-two confirmation of the whole span, the T5.N3/T5.N4 split, and the niche.


---

## Phase 5 exit — the D-gate-1 window, adjudicated (session U, 2026-08-15)

D-gate-1 suspended mid-phase perf measurement after session N, so **69 commits went
in unmeasured**: N's tail, sessions O, P, P′, P″, Q, R, S, T, and U's own work.
This is that window, opened after the session's last code commit (`e6873fe1`) so
the measurement window is closed. Every number below is a measurement (S33).

### Session nulls — and why both are printed

| pairs | decode median | decode band | width | encode median | encode band |
|---|---|---|---|---|---|
| 3 | +0.19% | +0.11% … +0.33% | 0.22 pt | +0.00% | −1.35% … +3.56% |
| 7 | **−0.11%** | **−0.74% … +1.82%** | **2.56 pt** | +0.00% | −4.65% … +2.67% |

**The 7-pair null is twelve times wider than the 3-pair one**, which is session K's
result arriving from the other side: that session judged a 7-pair verdict against a
3-pair band and S2b gained "the null must be run at the verdict's pair count". Here
the tight band is the 3-pair one, and taking it as the floor would have made
everything below look enormous. Every verdict in this section is judged against the
**7-pair** band.

### The owed reading — session N's stashed binaries, on a second day

N measured its span on 2026-08-13 at 7 pairs and owed a day-two confirmation that
sessions O–T never paid. `.perfpair/n_base|n_mid|n_head` were still on disk.

| span | N, day one (7 pairs) | U, day two (3 pairs) |
|---|---|---|
| **whole** `16a6130c`→`d0b7f399` | CB +1.24 / Main +1.90 / High +0.75, med **+1.24%** | CB +0.99 / Main +0.87 / High +0.57, med **+0.87%** |
| Face 1 `16a6130c`→`9da4bede` | CB **−0.72** / +0.11 / +0.13, med +0.11% | CB **+1.31** / +0.45 / +0.61, med +0.61% |
| Face 3 `9da4bede`→`d0b7f399` | CB +1.17 / **+2.25** / **+2.06**, med +2.06% | CB +1.64 / **+0.93** / **+1.78**, med +1.64% |

**The whole span survives its second day: N cost something, ≈ +0.87…+1.24%.** Both
readings put every decode row above the ceiling of their own session's null.

**The bisect does not survive it, and that matters more than the confirmation.**
N's day one read Face 1 at **−0.72% CB** and concluded "Face 1 is free and Face 3
owns all of it". Day two reads Face 1 at **+1.31% CB** — the sign flipped — and its
median lands above the null ceiling. S2b's clause is exact here: *two measurements
of one span disagreeing in sign is evidence the effect is below the measurement
error.* The halves were never resolvable; only the whole was.

**And the observation the niche hypothesis rested on is gone.** Day one's Face 3 had
CB +1.17% against Main +2.25% and High +2.06% — CB roughly half, which is what
pointed at `Option<PicId>` equality on B slices, since CB has no B-frames. Day two's
Face 3 reads CB **+1.64%**, Main +0.93%, High +1.78%: CB is no longer the cheap row.
**The asymmetry was noise.** The hypothesis was written down honestly as unverified
with one supporting observation; the observation did not replicate.

### The niche verdict — directionally consistent, not resolved

`74d02058` (T5.O0) is `n_head` plus two docs commits plus the niche, so it isolates
it exactly as session N's experiment specified.

| pairs | CB (no B-frames — the control) | Main | High | median |
|---|---|---|---|---|
| 1 | **−6.84%** | −0.97% | +0.37% | −0.97% |
| 3 | **+0.15%** | −0.72% | −0.99% | −0.72% |

**The one-pair reading is an artifact and its own control says so.** −6.84% on the
stream with no B-frames, where `Option<PicId>` equality cannot run, is not a
mechanism.

The supporting number, taken properly: `n_head`'s CB row was measured **nine times
today**, in one slot or the other of six different runs, and it spans **2.546 ms to
2.706 ms — a 6.3% range on one unchanged binary.** The one-pair reading's 2.706 is
the top of that range, not an outlier outside it, and that is the stronger version
of the point: when a binary's own readings spread 6.3%, a single pair cannot resolve
a sub-1% effect, and −6.84% is that spread being reported as a result. (S1 says two
runs of one binary drift ~3%; this is the same statement measured on this machine
today, at twice the size.) The brief asked for one pair; three were run because the
first disagreed with its own control.

At 3 pairs the control behaves — **CB +0.15%, flat, as a B-slice mechanism must be
on a stream without B-slices** — and Main/High read −0.72% and −0.99%. That is the
right sign and the right rows. It is **inside the 7-pair null band**, so it is not
resolved: the honest verdict is *directionally consistent, unresolved*, and the
mechanism it was proposed to fix has itself evaporated with the asymmetry above.

### The window — the phase's unmeasured span, measured

| span | pairs | CB (CAVLC) | Main | High | decode median | encode median |
|---|---|---|---|---|---|---|
| **`d0b7f399` → `e6873fe1`** (69 commits) | 3 | **+3.87%** | +2.63% | +2.55% | **+2.63%** | +0.10% |
| **`d0b7f399` → `e6873fe1`** | 7 | **+3.58%** | +2.77% | +2.21% | **+2.77%** | +0.21% |
| 7-pair null | 7 | +1.82% | −0.11% | −0.74% | −0.11% | +0.00% |

**Read twice, agreeing in sign, magnitude and row order, with every decode row above
the 7-pair null's ceiling. This is real.** Encode is flat at both counts, which is
what a decoder-only window must produce and is the check that the harness was not
simply having a bad day.

### The bisect — run because a stop-line breach demands it, and it does not resolve

| half | pairs | CB | Main | High | decode median | encode median |
|---|---|---|---|---|---|---|
| **A — structural**: O, P, P′, P″, Q, R (`d0b7f399`→`5fbe61a2`) | 3 | +1.13% | +1.36% | +2.07% | **+1.36%** | +0.00% |
| **B — parity + instruments**: S, T, U (`5fbe61a2`→`e6873fe1`) | 3 | +2.75% | −0.03% | −0.43% | **−0.03%** | +1.54% |

The CB rows sum (1.13 + 2.75 = 3.88 against the whole's 3.87/3.58) and the medians
do not (1.36 − 0.03 = 1.33 against the whole's 2.63/2.77). One of those is a
coincidence and **this harness cannot say which** — session N's own bisect note said
the same thing about the same instrument, and session K proved it at 7 pairs three
times. The whole span's number is the one to carry; the halves are worth less.

**What can be said about half B is an argument, not a measurement, and is labeled as
such:** sessions S, T and U changed error-signal bookkeeping, wrapped `DECODING_STATE`
in a `#[repr(transparent)]` newtype, fixed two one-line parity defects, added three
`deny` attributes and built instruments. None of that is per-macroblock work. A
+2.75% CB cost from it is not mechanically plausible, and its own decode median is
−0.03%. But the number is what it is, and "implausible" is not a measurement.

### The stop-line verdict — **OVER, and this escalates**

| term | CB |
|---|---|
| cumulative at N's head (Phase 4a exit +17.8%, the 5.2 flip, session N) | **+21.6 … +21.9%** |
| the D-gate-1 window (7 pairs / 3 pairs) | **+3.58 … +3.87%** |
| **cumulative CB now** | **≈ +25.2 … +25.8%** |
| stop-line (plan §7.4) | ≈ **+23%** |
| **breach** | **≈ 2.2 … 2.8 points over** |

D-perf-4's *other* tripwire — +25% **median** cumulative — is **not** breached: the
decode median has never tracked CB, which is the worst of the three streams and the
one the stop-line was written against. Both are stated so the escalation is not
argued from the harsher number alone.

The rule (plan §7.4) is *stop at a family boundary and escalate with the data*. The
tree is at a family boundary: W6 and W7 are unstarted structural work, not a
half-landed conversion. **This is that escalation.**

### The escalation table — options with measured sizes, for Eugene

| # | option | measured size | what it costs | what it risks |
|---|---|---|---|---|
| 1 | **Accept and re-baseline the stop-line to ≈+26%** | 0 work | nothing now | the next phase inherits no headroom at all; W6/W7 are still unmeasured and are the phase's largest structural work |
| 2 | **Day-two the window before deciding anything** | one session's bench time, no code | ~1 hour | nothing — and it is the only option that is free. The window is a **day-one** reading; every prior Phase 5 decision that rested on one reading (N's bisect, the niche asymmetry) has been overturned by its second day |
| 3 | **Land the niche's remaining recovery** | already in the tree since T5.O0; measured −0.72/−0.99% Main/High, +0.15% CB, inside the null | 0 | it does not touch CB, which is the row that breaches |
| 4 | **Bisect the window properly** (per-session builds, 7 pairs, two days) | 8 builds, ~16 runs | ≈ half a session of bench time | the 3-pair bisect above says the halves are at this harness's resolution limit; a per-session split is finer, so it is likely to resolve *less*, not more (session K's law) |
| 5 | **Park a conversion and re-measure** | not sized — option 4 is its prerequisite | unknown | nothing can be parked until something is attributed, and nothing is attributed |
| 6 | **Defer recovery to the Phase 9 perf pass** (D-perf-4's own disposition) | 0 now | the debt compounds through Phases 6–8 | the ledger already carries Phase 2's +14.7%/+16.6% on the same basis, so this is the precedent rather than a new policy |

**The recommendation, stated as one:** option 2 first, then re-read this table. The
breach is 2.2–2.8 points; the window's own two readings differ by 0.29 points and
the 7-pair null is 2.56 points wide. A second day is the cheapest thing that can
change the verdict, and this phase has twice had a second day overturn a conclusion
drawn from a first.

### The ledger, reconciled

No ledger row moves. The open rows are Phase 2's shim families, downgraded to
Phase 5 and then to the Phase 9 perf pass under D-perf-4, and **the window's cost is
not shim scaffolding** — it is the structural conversion work Phase 5 exists to do,
which has no recovery row because nothing was parked to recover. The recovery
expectation the phase carried was Phase 4a's de-virtualization, already banked in
the +17.8% base. So the reconciliation is: **cumulative CB ≈ +25.2…+25.8%, no row
recovers it, and the stop-line is breached** — which is why this is an escalation
and not a ledger entry.

### Day two — owed, and named

This session could not span days. Under S2b every reading above that a decision
rests on is therefore **provisional**, and the hand-off names exactly what day two
re-runs:

1. **The window at 7 pairs** (`n_head` → `u_head`) with a **7-pair null taken the
   same day** — the stop-line verdict rests on this and nothing else.
2. **The niche at 3 pairs** (`n_head` → `o_niche`) — to close it as resolved or
   inside the floor, on a day when the floor is known.
3. *Not* the bisect halves: they are at the resolution limit and a second day of
   them buys a second unresolvable answer (session K's law, stated here so the next
   session does not spend a morning proving it again).

Binaries are stashed and stay on disk: `.perfpair/n_base` (`16a6130c`), `n_mid`
(`9da4bede`), `n_head` (`d0b7f399`), `o_niche` (`74d02058`), `r_head` (`5fbe61a2`),
`u_head` (`e6873fe1`). Day two needs **no build**.

### Day two — paid, and the verdict does not move (session V, 2026-08-15)

**The separation, stated exactly rather than implied by a heading.** U's readings
were taken 2026-08-15, 09:30–10:30 PDT; these were taken the same day, 20:01–20:08
PDT — **≈10 hours later, same calendar day**, machine otherwise idle, no build in
between (all six binaries were already stashed). S2b's clause asks for a *different
day*, and this is not one; it is the widest separation Eugene's closing order left
available. It is a genuine re-roll of machine state — different thermal history,
different background load, a different page cache — and it is weaker than a true
day two. Every verdict below therefore rests on whether the two readings **agree**,
not on the second one alone.

#### The window — confirmed, and no longer provisional

| reading | pairs | CB (CAVLC) | Main | High | decode median | encode median |
|---|---|---|---|---|---|---|
| U, morning | 7 | +3.58% | +2.77% | +2.21% | **+2.77%** | +0.21% |
| **V, evening** | 7 | **+3.38%** | **+2.49%** | **+2.65%** | **+2.65%** | **+0.00%** |
| U's 7-pair null | 7 | +1.82% | −0.11% | −0.74% | −0.11% | +0.00% |
| **V's 7-pair null** (`u_head` both slots) | 7 | **+2.29%** | **+0.15%** | **−0.41%** | **+0.15%** | **+0.00%** |

Two independent 7-pair readings of `d0b7f399`→`e6873fe1`, agreeing in **sign**, in
**magnitude** (medians 0.12 points apart, CB rows 0.20 apart) and in **row order**
(CB worst in both), with every decode row above its own reading's null ceiling and
encode flat in both. **The window is real**, and the label "provisional" comes off
it.

**The margins are thinner in the evening and that is worth printing.** V's null CB
row is +2.29%, so CB clears its own null's ceiling by 1.09 points and Main by 0.20.
A single evening reading judged against a 2.70-point-wide band would not have
carried a verdict on its own. What carries it is the agreement between two
readings, which is exactly what S2b's day-two clause is for.

**And the recommendation the table made has now been spent.** Option 2 was "day-two
the window before deciding anything", on the ground that this phase had twice had a
second day overturn a first (N's bisect, the niche's asymmetry). It was the right
call to make and it did not overturn anything: the third second-day reading in this
phase is the first one that **confirms**. That is a result about the instrument as
much as about the window — the spans that survived their second day (N's whole span,
this window) are the ones measured whole, and the ones that did not (N's bisect
halves, the niche's asymmetry) are the ones measured in pieces at the resolution
limit.

#### The niche — inside the floor, and "directionally consistent" does not survive

| reading | pairs | CB (no B-frames — the control) | Main | High | median |
|---|---|---|---|---|---|
| U | 3 | +0.15% | −0.72% | −0.99% | −0.72% |
| **V** | 3 | **−0.15%** | **+0.33%** | **−0.29%** | **−0.15%** |

Main flips sign between the two readings (−0.72% → +0.33%) and High falls to a
third of its morning value. Every row of both readings is inside the 7-pair null
band on either day. U's verdict was *directionally consistent, unresolved*; the
second reading removes the first half of it, by S2b's own clause — two measurements
of one span disagreeing in sign is evidence the effect is below the measurement
error. **The niche is unresolved and inside the floor.** It stays in the tree where
T5.O0 put it: no row is outside the floor in either direction, so there is nothing
to recover and nothing to revert.

Not run: the bisect halves (session K's law; the hand-off said so and it was right).

#### The stop-line verdict, re-derived on two readings — **the breach stands**

| term | CB |
|---|---|
| cumulative at N's head (Phase 4a exit +17.8%, the 5.2 flip, session N) | +21.6 … +21.9% |
| the D-gate-1 window, 7 pairs, **two** readings | **+3.38 … +3.58%** |
| **cumulative CB now** | **≈ +25.0 … +25.5%** |
| stop-line (plan §7.4, D-perf-5's direction) | ≈ **+23%** |
| **breach** | **≈ 2.0 … 2.5 points over** |

The window's second reading is 0.20 points cheaper than its first, so the breach
narrows from ≈2.2–2.8 points to ≈2.0–2.5. It does not close, and no reading
available to this session can close it: the whole distance to the line is 2 points
and the window is the only unmeasured term left in the chain.

D-perf-4's *other* tripwire — **+25% median cumulative** — is **not** breached, on
the same basis U stated: the decode median has never tracked CB, and CB is both the
worst of the three streams and the row the stop-line was written against. Both are
stated so the escalation is not argued from the harsher number alone.

#### The disposition — **D-perf-6: recovery deferred to the Phase 9 perf pass**

Recorded in full at plan §7.4 as **D-perf-6**. In short: the escalation table's
**option 6**, which is D-perf-4's own disposition for a cumulative deficit that
nothing attributes, on the precedent the ledger already carries (Phase 2's +14.7% /
+16.6% shim families, downgraded to Phase 5 and then to the Phase 9 pass). Options
4 and 5 are refused on measurement grounds rather than on cost: the window's bisect
does not resolve at 3 pairs and a per-session split is finer, so it would resolve
less (session K's law), and **nothing can be parked because nothing is attributed**.
Option 1 (re-baseline to ≈+26%) is not taken — the line stays where D-perf-5 put it,
and the phase exits over it with the overage named.

**The ledger does not move.** No row recovers this: the window's cost is the
structural conversion work Phase 5 exists to do, which has no recovery row because
nothing was parked to recover. The new line for the record: **cumulative CB
≈ +25.0…+25.5% at Phase 5's exit, ≈2.0–2.5 points over the ≈+23% stop-line,
deferred to Phase 9 by D-perf-6.**

### Session V's own span — inside the floor, and nothing rests on it

Taken after V's last code commit, 2026-08-15 evening, the same sitting as the day two
above. Four commits, of which three carry code: F50's one-line `ParseSps` arm, F51's
restored `UninitFmoList` call at `ResetFmoList`, and three modules converted to
references and marked `#![deny(unsafe_code)]`. No kernel, no allocation path, no
dispatch change, no shim retirement.

| span | pairs | CB (CAVLC) | Main | High | decode median | encode median |
|---|---|---|---|---|---|---|
| `e6873fe1` → `92d6fa75` | 3 | +0.11% | −0.11% | +0.26% | **+0.11%** | −0.39% |
| 3-pair null (`v_head` both slots) | 3 | −0.04% | +0.46% | −0.15% | **−0.04%** | +2.88% |

**Every decode row of the span is inside the null band** (span −0.11%…+0.26% against
a null of −0.15%…+0.46%), so there is no effect to report and **no day two is owed**:
S2b's day-two clause attaches to readings a decision rests on, and no decision rests
on this one. Both binaries stay stashed (`.perfpair/u_head` = `e6873fe1`,
`.perfpair/v_head` = `92d6fa75`) so a later session can take one for free if it wants
the span in a chain.

The encode null's +2.88% median and −5.65%…+13.11% band are the ordinary reminder of
why encode rows are read against their own floor and not at face value (S2, and
Phase 3's exit at +22.57%).
