# SIMD Phases 1–4 — outstanding review findings

Review of commits `8ec12279`, `350d2846`, `8d385521`, `cece1fbf` (SIMD Phase 1–4):
22 files, +5311/−74, ~4,800 lines of hand-written x86_64 intrinsics plus dispatch wiring.

**Already fixed** (not repeated below): the tautological MC and IDCT parity tests, the
non-scalar `mc_luma_c` chain that made them impossible to fix by import swap alone, and the
aarch64 build break in `simd/mod.rs`.

**Verified clean** — worth recording so nobody re-audits it: the kernel arithmetic is
bit-exact. Independent oracles built from `codec/common/src/mc.cpp`, `deblocking_common.cpp`,
`get_intra_predictor.cpp` and the reference `.asm` were swept over MC (all 16 quarter-pel and
64 eighth-pel positions, widths/heights 1–17), deblocking (90,000 randomized trials, 417k
samples actually modified), quant (exhaustive: all 65,536 `i16` inputs × every `(ff, mf)` pair
in all 52 QP rows), and intra-pred/SAD/SATD (5 input distributions × 4 anchors). Zero
mismatches. Taps, rounding constants, saturation flavours, availability fallbacks, tc0 gate
polarity, transposes and lane orders are all correct. The findings below are in the
verification and dispatch layers, with one genuine arithmetic divergence (§1).

Severity ordering is by consequence, not by effort. "Latent" means the mechanism is confirmed
but no current caller reaches it.

---

## 1. IDCT vertical pass runs in `i16` where the scalar runs in `i32`

`rust/crates/openh264-rs/src/simd/x86_64/dct.rs:185` — **confirmed, live on the decoder path**

`compute_idct_residuals_sse2` does the whole vertical pass in 16-bit lanes:

```rust
let t1_a = _mm_add_epi16(s0, s8);
let t2_a = _mm_add_epi16(s4, _mm_srai_epi16(s12, 1));
let res0 = _mm_srai_epi16(_mm_add_epi16(_mm_add_epi16(t1_a, t2_a), c32), 6);
```

The scalar it dispatches away from widens first (`decoder/decode_mb_aux.rs:70,77`):

```rust
let s0 = src[i] as i32;  let s8 = src[i + 8] as i32;
res[0][i] = (32 + t1 + t2) >> 6;
```

The horizontal pass legitimately truncates to `i16` in both (documented at
`decode_mb_aux.rs:38-42` as load-bearing), so `src[]` spans the full `i16` range and
`s0 + s8` can overflow.

Worked example — `rs` all zero except `rs[0] = rs[8] = 20000`, prediction 0:

| | `t1` | `>> 6` | output |
|---|---|---|---|
| scalar | 40000 | 625 | `WelsClip1(625)` = **255** |
| SSE2 | wraps to −25536 | −399 | `packus` saturates to **0** |

Measured divergence rate: 0/50000 blocks at `|coef| <= 4000`, 170/50000 at 5000, 20074/50000
at 8000, 48132/50000 at 12000. A full encoder round-trip (FDCT → quant → dequant → IDCT) over
all 52 QPs × 20000 blocks produced **zero** mismatches, because encoder-side coefficient sets
stay consistent. The exposure is the **decoder**: `decoder_core.rs:1743` installs this as
`pIdctResAddPredFunc` and bitstream coefficients need not be consistent.

Upstream's own `SSE2_IDCT_4x4P` (`codec/common/x86/dct.asm:450`) is also 16-bit, so this
matches the asm — but not this port's scalar path. Since the aarch64 build was repaired, the
same stream now demonstrably decodes differently on aarch64 (always scalar, `i32`) than on
x86_64 (SSE2, `i16`).

**This needs a decision, not a patch.** Matching upstream's asm and matching the port's own
scalar path are mutually exclusive. Either:
- widen the vertical pass to `i32` (two `_mm_unpack*_epi16` + `epi32` arithmetic) so scalar and
  SIMD agree and the port diverges from upstream's asm; or
- keep 16-bit, and record the deliberate divergence in a comment plus a scalar-side note.

Whichever way, `encoder/decode_mb_aux.rs:121` currently still claims the vertical pass runs in
`i32` "total over the full coefficient range", which is now true only of the `_c` path that
comment sits above.

### 1a. The IDCT parity tests cannot see it

`rust/crates/openh264-rs/src/simd/x86_64/dct.rs:527`

The tautology is fixed, but `lcg_i16` still caps the generator:

```rust
((*seed >> 32) as i32 % 2000 - 1000) as i16   // |coef| <= 1000
```

That is a factor of five below the threshold where §1 appears. Widening it to the full `i16`
range is a one-line change — and it will turn §1 into a failing test, which is the correct
outcome, not a regression. Do §1 and §1a together.

---

## 2. Deblocking dispatch bypasses the `has_sse2()` gate

`rust/crates/openh264-rs/src/common/deblocking_common.rs:81`, `:147`, `:255`, `:294` — **confirmed**

All four deblocking dispatchers take the SIMD path unconditionally:

```rust
#[cfg(target_arch = "x86_64")]
{
    crate::simd::x86_64::deblock::deblock_luma_lt4_sse2(pix, step_x, step_y, alpha, beta, tc);
    return;                                   // always taken on x86_64
}
#[allow(unreachable_code)]                    // the allow admits it
deblock_luma_lt4_scalar(pix, step_x, step_y, alpha, beta, tc);
```

Every other dispatch site added by this series gates properly — `common/mc.rs:311,348,399,455`,
`decoder/decode_mb_aux.rs:44`, and the eleven sites in `encoder/decode_mb_aux.rs`:

```rust
#[cfg(target_arch = "x86_64")]
if crate::simd::has_sse2() { pixel_avg_sse2(...); return; }
pixel_avg_c(dst, a, b, width, height);
```

`simd/mod.rs:17` documents `OPENH264_NO_SIMD=1` as forcing "scalar fallbacks for testing and
differential verification". Under that env var every other kernel family falls back and all
four deblocking filters still run SSE2 — so a deblocking divergence can never be bisected to
SIMD-vs-scalar and will be misattributed to whichever kernel did fall back. `deblock_*_scalar`
is also unreachable in production on x86_64.

This is additionally the only place a safe `pub fn` calls `#[target_feature(enable = "sse2")]`
functions with no feature test (sound today only because SSE2 is x86_64 baseline).

**Fix:** add `if crate::simd::has_sse2()` to all four, matching the house pattern.

---

## 3. Two misaligned raw-pointer dereferences (UB)

`rust/crates/openh264-rs/src/simd/x86_64/intra_pred.rs:248` and `:452` — **confirmed**

```rust
let top = rec.row_n::<8>(-1, 0);              // [u8; 8], alignment 1
let top_u64 = *(top.as_ptr() as *const i64);  // requires alignment 8
```

and in `enc_i4x4_luma_pred_v_sse2`:

```rust
let top = rec.row_n::<4>(-1, 0);              // [u8; 4], alignment 1
let t_u32 = *(top.as_ptr() as *const i32);    // requires alignment 4
```

These are the **only two** reinterpreting raw derefs in the entire 4,800-line SIMD tree; every
other byte reinterpretation uses `from_ne_bytes` or `read_unaligned`. The correct form already
exists four lines away, in the decoder twins:

```rust
let top_u64 = u64::from_ne_bytes(top);   // intra_pred.rs:260
let t_u32   = u32::from_ne_bytes(top);   // intra_pred.rs:462
```

`Cargo.toml` sets `[profile.dev] debug-assertions = true`, which inserts rustc's alignment
check and aborts with `misaligned pointer dereference: address must be a multiple of 0x8`.
It survives today only because LLVM happens to over-align the stack slot — codegen luck, not a
guarantee, and it licenses the optimizer to assume alignment.

**Fix:** two lines, copy the decoder twins.

---

## 4. Nothing in CI executes the SIMD code

`rust/tools/gates.sh:267`, `:296` — **confirmed, partially improved**

`gates.sh` runs bare `cargo build --all-targets` and `cargo test "$@"`.
`grep -rn '\-\-target' rust/tools/gates.sh rust/tools/diffharness/*.sh` returns nothing, and
the dev host is `aarch64-apple-darwin`, where `simd/mod.rs:10` gates the whole tree out.

The build break is fixed, so the gates now run and cover the 435 scalar tests. But the 31 SIMD
parity tests — genuine as of this session — still never execute in the gate battery.

Separately, `grep -rn OPENH264_NO_SIMD rust/` returns exactly three hits, **all inside
`simd/mod.rs` itself** (lines 17, 20, 71). The kill switch has no caller in `compare.sh`,
`sweep.sh`, `inputs.sh` or `gates.sh`, so the harness never runs the scalar-vs-SIMD comparison
the switch exists for.

`rust/README.md:159` — "Byte parity is the definition of done".
`rust/README.md:213` — "Add the harness knob before the feature: a configuration the drivers
cannot express has no referee, and a port without a referee is a guess."

**Fix:** run the gates and the diffharness sweep under `--target x86_64-*`, and add an
`OPENH264_NO_SIMD=1` arm to `compare.sh`. Until then the port's stated definition of done is
not being evaluated against any of this work.

### 4a. The AVX2 test launders absent coverage

`rust/crates/openh264-rs/src/simd/x86_64/sad.rs:364`

```rust
fn test_avx2_sad_parity() {
    if !std::is_x86_feature_detected!("avx2") { return; }
```

The dev host is an Apple M1; Rosetta 2 provides no AVX2 (`sse2`/`ssse3`/`sse4.1` only). The
test skips every assertion and still prints `... ok`. The AVX2 kernels reached from
`encoder/sample.rs:172` are unverified in **every** configuration available in this repo while
the suite reports them green.

**Fix:** `#[ignore]` with a reason, or a hard failure when AVX2 is expected — not a silent
`return`.

---

## 5. Stack-buffer overflow in `mc_hor_ver22_inner_sse2` for `width >= 19`

`rust/crates/openh264-rs/src/simd/x86_64/mc.rs:552`, `:569`, `:578` — **confirmed, latent**

```rust
let mut iTmp = [0i16; 17 + 5];      // 22 elements
let n = width + 5;
...
while j + 8 <= n {
    _mm_storeu_si128(iTmp.as_mut_ptr().add(j) as *mut __m128i, res);   // no len() check
    j += 8;
}
```

At `width = 19`, `n = 24`: `j = 16` passes `16 + 8 <= 24` and the store writes elements 16..24
of a 22-element array — 4 bytes past the end of the stack frame, with no panic.

The scalar twin `mc_hor_ver22_c` (`common/mc.rs:409`) writes through `iTmp[..n].iter_mut()` and
panics cleanly; the C fixes `int16_t iTmp[17 + 5]` against `for (j = 0; j < iWidth + 5; j++)`.

Not reachable today: max caller width is 17 (`encoder/md.rs:1529`, `kiW + 1` with `kiW = 16`),
which fills `iTmp` to exactly 22 with zero headroom. But **this series deleted the comment that
recorded the precondition**:

> `iTmp` is `[i16; 17 + 5]` as in the C++, and `width` above 17 indexes past it — a panic here.

**Fix:** bound the loop by `iTmp.len()` as well as `n`, or restore an explicit assert. One
caller away from converting a panic into memory corruption.

---

## 6. Safe `pub fn` reaches an AVX2 kernel with no feature check

`rust/crates/openh264-rs/src/simd/x86_64/sad.rs:252`, `:262` — **confirmed soundness hole**

```rust
#[inline(always)]
pub fn sample_sad_16x16_avx2<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    unsafe { sad_16x_avx2::<S, 16>(sample1, sample2, 0, 0) }
}
```

`sad_16x_avx2` (`sad.rs:36`) is `#[target_feature(enable = "avx2")] pub unsafe fn`. `simd`,
`x86_64` and `sad` are all `pub mod`, so this is callable from entirely safe code and executes
`vpsadbw` on any pre-Haswell Intel or pre-Excavator AMD CPU: SIGILL. Cargo's unsafe discipline
gives no warning because the wrapper is safe.

The production dispatch at `encoder/sample.rs:172` does guard on `uiCpuFlag & WELS_CPU_AVX2` —
but that guard lives in a different module, with nothing in the wrapper's signature or docs
requiring it.

**Fix:** mark both wrappers `unsafe fn` with a stated precondition, or dispatch on
`is_x86_feature_detected!("avx2")` inside them.

### 6a. `sad_16x_avx2` reads a row past the block for odd `H`

`rust/crates/openh264-rs/src/simd/x86_64/sad.rs:45-62` — latent

The loop steps two rows at a time (`while y < H { ... y += 2; }`), so `H = 5` would accumulate
row 5. Only `H = 16` and `H = 8` are instantiated today, but the SSE2 twin at `:22`
(`for y in 0..H`) has no such constraint and neither signature documents the difference. Add
`const { assert!(H % 2 == 0) }`.

---

## 7. Vertical deblocking rewrites samples the scalar never touches

`rust/crates/openh264-rs/src/simd/x86_64/deblock.rs:540`, `:564`, `:631` — **plausible; mechanism confirmed, trigger uncertain**

```rust
for i in 0..16 { rows[i] = pix.row_n::<8>(i as isize, -4); }      // loads cols -4..+3
for x in 2..=5 { for y in 0..16 { rows[y][x] = t[x][y]; } }       // modifies only -2..+1
for i in 0..16 { pix.set_row_n::<8>(i as isize, -4, &rows[i]); }  // writes back all 8
```

The scalar writes only `b − 2·step_x`, `b − step_x`, `b`, `b + step_x`, and even those
conditionally (inside `if deta_p0q0 && deta_p1p0 && deta_q1q0`, `if deta_p2p0`, `if deta_q1q0`).
Same over-write in `eq4` (cols −4..+3 vs the scalar's −3..+2) and in the chroma kernels.

`encoder/deblocking.rs:587` calls this as `deblock_luma_lt4(pix, 1, iStride, ...)` with
`pix: &mut RecCursor<'_>` anchored at the macroblock origin, so columns −4 and −3 are the
**previous macroblock's** last two samples. `encoder/rec_view.rs:32` states writes "go through
`Cell` with **no synchronisation at all**", and `encoder/deblocking.rs` takes an `SSliceCtx`
for slice multi-threading.

Single-threaded this is value-neutral (the same bytes are written back). What would confirm it
is establishing whether two threads can deblock adjacent macroblocks concurrently. Independent
of threading, it widens the kernel's read/write contract versus the scalar it must match, and
the parity tests only filter well inside a padded 32×32 plane so they cannot see it.

### 7a. Undocumented stride precondition

`rust/crates/openh264-rs/src/simd/x86_64/deblock.rs:517`, `:579`, `:648`, `:733` — latent

The direction guards test `step_y == 1` / `step_x == 1`, but the bodies address via the
*cursor's* stride (`pix.row_n::<16>(-3, 0)`), silently adding an unchecked
`step_x == pix.stride()` precondition. The scalar is stride-agnostic by design —
`deblocking_common.rs:52-55` states the kernels "do all their addressing in flat byte offsets
via `at(off, 0)` / `set(off, 0, _)`". A caller passing `step_x = 2 * cursor.stride()` (field
addressing, or a cursor built over a different pitch than the `iStride` argument) still
satisfies `step_y == 1`, so the scalar fallback is *not* taken and the filter reads and writes
the wrong samples. No current caller violates it.

---

## 8. `has_sse2()` latches a stale answer that `detect_cpu_features()` does not

`rust/crates/openh264-rs/src/simd/mod.rs:71-80`, `rust/crates/openh264-rs/src/decoder/decoder_core.rs:1749` — **confirmed**

Two dispatch mechanisms coexist and can disagree:

- `detect_cpu_features()` re-reads `OPENH264_NO_SIMD` on **every** call.
- `has_sse2()` seeds a process-wide `SSE2_CACHED: AtomicU8` on first call and never re-reads.

`decoder_core.rs:1749` gates `pIdctResAddPredFunc` on the per-context `pCtx.uiCpuFlag`, while
the sibling slot `pIdctFourResAddPredFunc` reaches SIMD through `has_sse2()` — and that slot is
where `decode_slice.rs:913/931/1873/2030` route the bulk of 4×4 residual adds.

Concrete state: a host decodes one stream, then sets `OPENH264_NO_SIMD=1` and opens a second
decoder. It gets `uiCpuFlag == 0` (every table slot scalar, `InitMcFunc` scalar) but
`has_sse2() == true` from the stale cache — so the four-block path still runs SSE2 while the
single-block path runs scalar.

Combined with §2, **no per-context or env-based switch currently turns all SIMD off**, which is
the same capability §4 needs for differential verification.

**Fix:** thread the CPU flag from decoder/encoder init rather than reading process environment
in a codec hot-path module, or at minimum have `has_sse2()` consult the same latched value the
tables were built from.

---

## 9. `InitMcFunc`'s CPU-flag gate is dead code

`rust/crates/openh264-rs/src/common/mc.rs:1196` — **confirmed**

`InitMcFunc` installs `pfLumaHalfpelHor/Ver/Cen`, `pfSampleAveraging`, `pMcChromaFunc`,
`pMcLumaFunc` under `if (uiCpuFlag & WELS_CPU_SSE2) != 0`. Grepping all six field names across
`src/` yields no call site outside `common/mc.rs` — the only other references are the field
declarations (`decoder/decoder_context.rs:1281`, `encoder/wels_func_ptr_def.rs:327`) and the two
`InitMcFunc` calls. `encoder/md.rs:582` says it outright: "MC and the half-pel filters are
called directly, not via `sMcFuncs`."

So a host that restricts `uiCpuFlag` gets SSE2 anyway, and the new `init_mc_func_cpu_flags`
test asserts a gate with no runtime effect.

This is the altitude question behind §2 and §8: upstream dispatches through a function-pointer
table populated once at init (`codec/common/src/cpu.cpp` and the `InitFunctionPointers`-style
setup), and this port has that mechanism *and* 18 per-call `if has_sse2()` branches that bypass
it. Picking one would resolve §2, §8 and §9 together.

---

## 10. Kernels that are SIMD in name only

**`rust/crates/openh264-rs/src/simd/x86_64/quant.rs:278` and `:209`** — confirmed

`hadamard_t4_dc_sse2` is byte-identical to the scalar `encoder/encode_mb_aux.rs:525`
(diff shows only a comment line), and `dequant_ihadamard_4x4_sse2_impl` is identical to
`encoder/decode_mb_aux.rs:46-71`. Neither contains a single `_mm_*` call, yet the latter carries
`#[target_feature(enable = "sse2")] unsafe fn` and a wrapper spends an `unsafe {}` block on it.
Both are installed over the identical scalar in the dispatch tables
(`encode_mb_aux.rs:847`, `decode_mb_aux.rs:457`).

Critically, **neither has a parity test** — the quant test module covers the other seven kernels
only. A future edit to either scalar silently applies on non-x86_64 only.
`dequant_ihadamard_4x4_sse2` is live on the encoder's luma-DC reconstruction path for every
I16x16 macroblock at `qp >= 12`.

**Also:** eight `_sse2`-named intra-pred predictors contain no intrinsic at all —
`intra_pred.rs:258, 269, 280, 291, 326, 460, 485, 512`.

**Fix:** delete the two quant clones and their dispatch-table overrides, or implement them.
Either way the names should stop claiming vectorization that isn't there.

---

## 11. `~250` duplicated lines in `simd/x86_64/mc.rs`

`rust/crates/openh264-rs/src/simd/x86_64/mc.rs:626-912` — confirmed

`scratch()`, the twelve `mc_hor_verXY_sse2` quarter-pel wrappers and `mc_luma_sse2` are a
verbatim copy of the scalar composites with `_sse2` appended to the call names, containing no
SIMD of their own.

Note this changed character with the `mc_luma_c` fix: before, the scalar composites
self-dispatched, so the `_sse2` copies were a redundant second spelling. Now that the scalar
chain is genuinely `_c`, they are a real second chain — so collapsing them requires
`mc_luma`'s dispatcher to fan out over the `_sse2` leaves directly rather than simply deleting
the block.

Secondary cost while it stands: `scratch()` is called 21 times across the file, each
zero-initializing a 256-byte stack array the filter then fully overwrites, and `#[inline(never)]`
on the callers stops LLVM from eliminating the memset.

Comparable duplication in `intra_pred.rs`: the ten `enc_*`/`dec_*` predictor pairs are ~350 of
the file's 871 lines, differing only in how neighbours are read and rows addressed. `RefSamples`
is already implemented by both `RecCursor` and `PlaneCursorMut`, so one generic `_impl` per pair
plus two thin call sites would collapse them.

---

## 12. Performance items worth measuring before keeping

Flagged but **not benchmarked** — this host is arm64, so none of it could be timed natively.

- **`deblock.rs:543`** — the vertical-edge path wraps a ~40-instruction SIMD kernel in a fully
  scalar byte-by-byte 16×8 transpose, done twice, plus 32 row copies: roughly 450 scalar byte
  operations to feed 40 SIMD instructions. The standard
  `punpcklbw`/`punpckhbw`/`punpckldq`/`punpcklqdq` ladder is ~24 SSE2 instructions. The same
  gather/scatter is copy-pasted four times. Plausibly slower than the scalar it replaces.
- **`dct.rs:14`, `:142`** — both 1-D row transforms compute in scalar `i32` GPRs and reassemble
  with `_mm_set_epi16`, costing ~8 GPR↔XMM domain crossings per 4×4 block, ~128 per macroblock.
  Upstream keeps the horizontal pass entirely in registers (`SSE2_IDCT_HORIZONTAL`,
  `codec/common/x86/dct.asm:416`) — which is also why the asm never needs the scalar detour that
  forced the `i16` narrowing in §1.
- **`mc.rs:600`** — `mc_hor_ver22_inner_sse2` vectorizes only the vertical pass; the horizontal
  pass over the SIMD-produced `iTmp` is the unmodified scalar loop copied from `mc_hor_ver22_c`,
  including a per-output-pixel `w.try_into().unwrap()`. For 16×16 that is 256 scalar window
  slices per call, in the most-invoked half-pel mode.
- **`satd.rs:81`** — `satd_4x4_sse2_impl` uses only the low 64 bits of every 128-bit register, so
  every add/sub does half its work on zeroes; `satd_16x16` then performs 16 full horizontal
  reductions and 16 XMM→GPR moves. Loading 8 bytes per row with `_mm_loadl_epi64` would put two
  adjacent 4×4 blocks in one register and halve both counts.

---

## 13. Repo gates and documentation

- **`rust/tools/unsafe_baseline.json`** — `unsafe_ratchet.sh check` exits RC=1:
  `raw_ptr 391 → 591 (+200)`, `unsafe_block 122 → 246 (+124)`, `unsafe_fn 52 → 117 (+65)`, and
  the baseline was not regenerated in the series. In fairness the gate was **already** marginally
  red at `HEAD~4` (+2 blocks, +4 fns, raw_ptr +0), so this series did not turn it red — but it
  widens the breach by two orders of magnitude and buries the pre-existing drift.
  `rust/README.md:207`: "A count that must move for a deliberate change is regenerated in the
  same commit, with the reason."
- **`rust/tools/unsafe_census.txt`** — `unsafe_census.sh check` reports FAIL, listing
  `simd/x86_64/intra_pred.rs`, `sad.rs` and `satd.rs` as "present but unpinned". `sad.rs` and
  `satd.rs` carry no `// unsafe-cat:` tags at all, unlike `intra_pred.rs`.
- **`rust/crates/openh264-rs/src/lib.rs:12-17`** — `unsafe_op_in_unsafe_fn` was dropped from the
  crate-wide `#![deny(...)]` in Phase 4. **Verified gratuitous**: re-inserting it and building
  for x86_64 produces zero errors, because the two modules that need the relaxation already
  scope it themselves (`deblock.rs:6`, `intra_pred.rs:6`). Restoring it is free and re-arms the
  invariant for all ~100 non-SIMD modules.
- **`rust/README.md:24`, `:186-187`** — still documents the port as
  `| SIMD | none; the port is scalar throughout |`. Line 22's "70 of 87 source files are
  `#![forbid(unsafe_code)]`" is stale by exactly the nine new SIMD files (now 70 of 96).
- **Stale in-code invariants**, more load-bearing than the README because they license
  optimizations:
  - `encoder/sample.rs:116` — "**This is the only writer of the three tables, and `_uiCpuFlag`
    is unused** — so every slot is a compile-time constant … and a call site whose block index is
    itself a constant may call the kernel directly, byte-identically, without going through the
    table at all." Now false (`sample.rs:151`, `:171` make the flag live), and ~20 call sites do
    bypass the table.
  - `encoder/encoder_context.rs:1724` — "this target reports no CPU features
    (`WelsCPUFeatureDetect` returns 0), so only the `_c` kernel is ever assigned".
  - `encoder/get_intra_predictor.rs:9`, `encoder/svc_base_layer_md.rs:19` — both still assert the
    port has no SIMD.
- **`simd/mod.rs:6`** — the subtree blanket-allows `dead_code`, a lint the crate root
  deliberately does not suppress. It is the one lint that would report a kernel wired to nothing.
  (Checked: no kernel is currently orphaned — all 110 `pub fn`s are reachable from a dispatch
  site — so this is a removed safety net rather than a live bug.)

---

## 14. Coverage gaps in `dct.rs`

`rust/crates/openh264-rs/src/simd/x86_64/dct.rs` — nine of fourteen public entry points have no
test: `idct_t4_rec_to_view_sse2` (`:322`), `idct_four_t4_rec_to_view_sse2` (`:350`),
`idct_t4_rec_in_place_view_sse2` (`:373`), `idct_four_t4_rec_in_place_view_sse2` (`:390`),
`idct_t4_rec_on_mb_in_place_view_sse2` (`:407`), `idct_rec_i16x16_dc_to_view_sse2` (`:497`),
`idct_four_t4_rec_sse2` (`:278`), `idct_four_t4_rec_in_place_sse2` (`:299`),
`idct_t4_rec_in_place_sse2` (`:255`).

These are not dead — `encoder/svc_encode_mb.rs:472/485/577` and
`encoder/svc_encode_slice.rs:1581/1595/1648/1652/1656` reach them through the wrappers at
`encoder/decode_mb_aux.rs:255/278/298/319/337/356`. Each carries a hand-written `off`/`advance`
offset (e.g. `dct.rs:342`, `:400`) where a transposed `(dx, dy)` or stride mix-up would be
silent. This is the encoder's reconstruction seam.

Similar: `intra_pred.rs` has 3 tests for 30 public kernels, `sad.rs` 3 for 23.

---

## Suggested order

1. §3 (two lines, removes UB), §5 (restores a deleted guard), §6 (closes a soundness hole).
2. §2 + §8 + §9 together — one dispatch mechanism instead of three; this is what unblocks §4.
3. §4 + §4a — get the gates running the SIMD tests at all. Without this the repaired parity
   tests protect nothing in CI.
4. §1 + §1a — decide the `i16`/`i32` question, then widen the generator so the decision is
   enforced.
5. §13 documentation and gate baselines — cheap, and several of the stale comments actively
   mislead.
6. §10, §11, §7, §12, §14 as cleanup capacity allows.
