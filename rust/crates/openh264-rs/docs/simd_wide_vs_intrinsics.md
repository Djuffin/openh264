# SIMD kernels: `core::arch` intrinsics versus the `wide` crate

Measured 2026-09-04 on an Intel Core i7-14700F (Windows 11, rustc 1.98.1, `wide` 1.3.0,
release profile, default x86_64 target features, one encoder thread).

## What was built

`src/simd/x86_64/` holds the port's hand-written SSE2/AVX2 kernels: 63 entry points
across nine families (SAD, SATD, motion compensation, DCT/IDCT, quantisation, CAVLC
scoring, block copy, intra prediction, deblocking). `src/simd/wide/` is a second,
complete implementation of the same 63 entry points written against the `wide`
crate's portable lane types (`u8x16`, `i16x8`, `i32x4`, ...). Every file in it is
`#![forbid(unsafe_code)]`.

The two modules export identical names and signatures. `simd::kernels` aliases one of
them, and all 71 dispatch sites in the codec call through the alias, so the whole
codec runs on one set or the other:

```text
cargo build --release                  # simd::kernels = simd::x86_64 (intrinsics)
cargo build --release --features wide  # simd::kernels = simd::wide
```

Both modules are compiled whenever they can be; the feature only moves the alias.
`wide` is an optional dependency, so the default library still has none.

### Porting rules, so the comparison measures the API and not the author

- Same name, same signature, same data access (`row_n`, `row_view`, `block_span`),
  so both sides pay the same bounds checks.
- Same algorithm, including where the intrinsic kernel does a step in scalar (the
  forward DCT's row pass, the IDCT's row pass).
- Restructured only where `wide` cannot spell the intrinsic's operation, and each
  such place is named in the source. The list:

| missing in `wide` 1.3 | what the port does instead |
|---|---|
| `psadbw` (byte abs-diff sum) | `max - min` on bytes, two zero-extends, adds, `pmaddwd`-against-ones reduce |
| `pavgb` (rounded byte average) | `(a \| b) - ((a ^ b) >> 1)` as a word shift with the cross-byte bit masked |
| integer lane permutes below SSSE3 (`swizzle` is `pshufb`) | array casts through `bytemuck`, lowered by LLVM to `pshufd`/`pshuflw`/`punpck` |
| runtime feature dispatch | none: `u8x32` is two SSE2 halves unless the whole crate is built with `+avx2`; the AVX2 table slots get a two-rows-per-step 128-bit kernel |
| a 16-byte load from `&[Cell<u8>]` | the block copies read cells one at a time, with the span check hoisted |

## Correctness

- The 59 parity tests from the intrinsic module (every kernel against its scalar
  reference, over several input distributions and anchors) run unchanged against the
  wide module and pass.
- The full suite passes with the wide kernels dispatched: 548 library tests plus every
  integration suite, including the 58 conformance streams and the byte-parity
  suites. It also passes on the default build after the dispatch-site rename.
- Both benches check bit-exactness: the kernel bench compares checksums of the three
  implementations' outputs on every row, the encoder bench compares SHA-1 of the
  bitstreams across the three sides. All rows and all six clips agree.

## Per-kernel cost

`cargo bench --bench kernel_bench --features wide`. Nanoseconds per call, best of 7
blocks of about 20 ms each, inputs made opaque per call with `black_box` (without
that, LLVM hoisted whole wide kernels out of the timed loop — the fully inlined,
safe code is that transparent to it). Ratios above 1 mean the second implementation
is faster.

```text
 kernel                                    scalar    intrin      wide   intrin/sc   wide/sc wide/intr
 sad 16x16                                   14.2      10.3      13.2       1.38x     1.07x     0.78x
 sad 8x8                                      7.9       6.8       5.6       1.16x     1.41x     1.22x
 sad 4x4                                      5.3       3.4       3.0       1.58x     1.79x     1.13x
 sad-four 16x16                              52.3      27.1      41.2       1.93x     1.27x     0.66x
 sad-four 8x8                                27.2      14.7      19.0       1.86x     1.43x     0.77x
 satd 4x4                                    18.9       5.5       5.8       3.41x     3.26x     0.95x
 satd 8x8                                    75.8      22.5      22.9       3.37x     3.32x     0.99x
 satd 16x16                                 302.7      87.5      95.4       3.46x     3.17x     0.92x
 mc pixel_avg 16x16                          63.0      38.6      36.4       1.63x     1.73x     1.06x
 mc hor_ver20 16x16                         109.7      65.9      69.9       1.66x     1.57x     0.94x
 mc hor_ver02 16x16                         109.6      70.8      34.8       1.55x     3.15x     2.04x
 mc hor_ver22 16x16                         294.6     334.9     363.3       0.88x     0.81x     0.92x
 mc hor_ver02 8x8                            48.8      19.8      13.6       2.46x     3.58x     1.45x
 mc luma qpel(1,3) 16x16                    267.6     178.4     147.4       1.50x     1.82x     1.21x
 mc chroma (3,5) 8x8                         47.6      19.0      13.9       2.50x     3.43x     1.37x
 mc chroma (3,5) 4x4                         12.2      11.3       9.0       1.08x     1.35x     1.25x
 dct 4x4                                      9.7       7.3       8.7       1.34x     1.12x     0.83x
 dct four 4x4                                42.4      31.3      38.2       1.35x     1.11x     0.82x
 idct t4 in place                            14.6       7.8       7.7       1.88x     1.88x     1.00x
 idct res_add_pred (decoder)                 12.3       7.7       7.8       1.60x     1.58x     0.99x
 idct i16x16 dc                             106.0      22.7      24.2       4.68x     4.38x     0.94x
 quant 4x4                                    5.8       1.5       0.9       3.92x     6.28x     1.60x
 quant four 4x4 max                          36.0       6.7       4.5       5.40x     8.08x     1.50x
 dequant four 4x4                             2.2       2.6       1.9       0.86x     1.20x     1.40x
 hadamard t4 dc                               6.9       4.3       3.1       1.59x     2.23x     1.40x
 dequant ihadamard 4x4                        4.2       8.5       5.7       0.50x     0.75x     1.51x
 get_none_zero_count                          1.1       1.7       2.0       0.64x     0.55x     0.86x
 calculate_single_ctr 4x4                     5.2      10.4       6.9       0.50x     0.76x     1.50x
 copy 16x16                                   3.6       7.6       3.5       0.47x     1.00x     2.15x
 copy 8x8                                     3.3       3.2       2.0       1.03x     1.59x     1.55x
 intra 16x16 plane                           61.3      19.6      19.6       3.13x     3.12x     1.00x
 intra 16x16 dc                               6.3       5.3       5.7       1.18x     1.10x     0.93x
 intra chroma plane                          20.4       7.7       4.9       2.64x     4.12x     1.56x
 intra 4x4 dc                                 2.0       1.5       1.5       1.38x     1.38x     1.00x
 intra 4x4 ddl                                3.1       1.3       1.3       2.37x     2.31x     0.97x
 deblock luma lt4, horizontal edge           68.9      20.5      21.3       3.37x     3.23x     0.96x
 deblock luma eq4, horizontal edge           66.1      22.5      22.3       2.93x     2.96x     1.01x
 deblock chroma lt4, horizontal edge         40.6      22.8      18.1       1.78x     2.25x     1.26x
 deblock luma lt4, vertical edge             68.9      44.8      44.5       1.54x     1.55x     1.01x
 deblock luma eq4, vertical edge             66.0      53.4      53.0       1.24x     1.25x     1.01x
 deblock chroma lt4, vertical edge           40.5      36.7      31.6       1.10x     1.28x     1.16x
 geometric mean of the ratios                                               1.65x     1.85x     1.12x
```

Reading it by family:

- **SAD is where the intrinsics win, by 1.3-1.5x on the 16-wide shapes.** This is
  the `psadbw` gap: the intrinsic row is one instruction plus an add; the wide row is
  seven. The four-point SAD, the motion search's inner loop, is the worst row in the
  table for wide (0.66x).
- **SATD, the deblocking filters, the IDCTs and the plane predictors are a wash**
  (0.92x-1.01x). These are word-lane arithmetic, which `wide` spells directly, and
  the array-cast permutes in the SATD lowered to real `pshufd`/`pshuflw` (see the
  codegen notes).
- **Wide wins on the vertical MC filter (2.0x), chroma MC, the quantisers and the
  Hadamards (1.4-1.6x).** Two reasons. Wide's `hor_ver02` narrows and stores sixteen
  bytes at once where the intrinsic kernel does two eight-byte halves; and for the
  small quantiser kernels the intrinsic `#[target_feature]` bodies are separate
  functions LLVM chose not to inline, so the intrinsic side pays a call the fully
  inlined wide side does not.
- **The forward DCT loses 17%**: its scalar row pass, rebuilt from `to_array()` and
  `i16x8::new`, lowers to twelve `pextrw`/`pinsrw` pairs where the intrinsic's
  `movd` extraction is shorter.
- Rows where the scalar beats both (the centre filter `hor_ver22`, `dequant
  ihadamard`, `get_none_zero_count`, `calculate_single_ctr`) are rows where the
  intrinsic kernel was already not a win; the wide port inherits that faithfully.

## Whole encoder

`cargo bench --bench simd_vs_scalar_bench` with `BENCH_WIDE_EXE` pointing at the
`--features wide` build of the same bench. Frames per second, one thread, best of two
passes, six ffmpeg-generated clips, bitstreams SHA-1-identical across all three sides.

```text
 configuration                  frames  scalar fps   SIMD fps   wide fps   SIMD/sc  wide/sc wide/SIMD
 320x240 QVGA high-contrast        200      1306.7     2139.2     1993.9     1.64x    1.53x     0.93x
 320x240 QVGA mandelbrot           200       604.9      999.0      925.7     1.65x    1.53x     0.93x
 640x480 VGA SMPTE bars            100      1999.9     2225.1     2157.5     1.11x    1.08x     0.97x
 640x480 VGA mandelbrot            100       243.8      392.5      362.6     1.61x    1.49x     0.92x
 1280x720 720p SMPTE bars           50       509.5      572.0      569.1     1.12x    1.12x     0.99x
 1280x720 720p mandelbrot           50       123.1      189.4      176.1     1.54x    1.43x     0.93x
 mean (ratios are geometric)                 798.0     1086.2     1030.8     1.42x    1.35x     0.95x
```

The wide build keeps about 88% of what the intrinsics buy over scalar (1.35x against
1.42x) and runs 5% slower than the intrinsics end to end. That is the SAD family:
motion search is the encoder's hot loop and it is the one place the portable API
has no equivalent instruction. The intrinsic build also has the AVX2 16x16/16x8 SAD
kernels installed on this CPU; the wide build cannot, because `wide` has no runtime
dispatch.

## Codegen notes

From `cargo rustc --release --features wide --example simd_probe -- --emit asm`
(`examples/simd_probe.rs` wraps each kernel pair in a `#[no_mangle]` function):

| kernel | intrinsics | wide | note |
|---|---|---|---|
| sad 16x16 | 69 instr | 81 instr | +2 `punpck`, +1 `pshufd` per row for the abs-diff widening |
| satd 4x4 | 208 instr, 64 shuffles | 185 instr, 65 shuffles | the array-cast permutes became `pshufd`/`pshuflw`/`shufps`; the bulk of both is the eight bounds-checked 4-byte row gathers |
| dct 4x4 | 215 | 286 | 12 `pextrw` + 12 `pinsrw` for the scalar row pass |
| hor_ver02 16x16 | 234, 54 vector loads/stores | 203, 49 | one 16-byte narrow and store per row instead of two 8-byte |
| dequant ihadamard | tail call | 130 | the 4x4 word transpose lowered to `and`/`andn`/`or`/`shufps` rather than four `punpck`; 16 of the 41 vector memory ops are Windows ABI xmm6-15 saves |

Every wide kernel inlined completely into its caller; the intrinsic
`#[target_feature]` `_impl` bodies stayed out of line in three of the eight probes.

## Conclusions

1. **Safe, portable SIMD through `wide` recovers nearly all of the intrinsics' gain
   on this codec**: 1.85x over scalar per kernel (intrinsics 1.65x) and 1.35x on the
   encoder (intrinsics 1.42x), with zero `unsafe` and no per-function
   `target_feature` boundaries.
2. **The residual is one instruction.** Without `psadbw`, byte SAD costs seven ops
   per row instead of two, and SAD is what the encoder spends its time on. A `wide`
   wrapper for it (or a hybrid: intrinsics for SAD, `wide` for everything else) would
   close most of the 5% gap.
3. **`wide` 1.3's integer surface has real holes**: no byte average, no integer lane
   permutes on the SSE2 baseline, no byte-to-word widening on the 256-bit types, no
   runtime dispatch. Everything else the codec needed was a direct call.
4. The per-kernel table is a better guide to the API than the whole-encoder number,
   and the whole-encoder number is the one that matters for shipping.

## Reproducing

```bash
cd rust/crates/openh264-rs
cargo test --release --features wide                       # full suite on the wide kernels
cargo bench --bench kernel_bench --features wide           # per-kernel table
cargo bench --no-run --features wide --bench simd_vs_scalar_bench   # note the exe path
BENCH_WIDE_EXE=<that path> cargo bench --bench simd_vs_scalar_bench # whole encoder
```
