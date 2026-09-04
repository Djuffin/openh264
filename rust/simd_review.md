# SIMD Phases 1–4 — review findings, all addressed

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

# Status

All fourteen findings below are addressed. The original analysis is kept verbatim;
each section now opens with what was done. `cargo test` is green — 28 test binaries,
483 lib tests (was 466) plus the integration suites, including the 60 decoder
conformance streams and the 16 malformed-stream golden tables, none of which needed
regenerating.

| | finding | resolution |
|---|---|---|
| 1 | IDCT vertical pass in `i16` | **fixed** — widened to `epi32`; deliberate divergence from upstream's SSE2 asm, recorded |
| 1a | parity tests capped at `\|coef\| <= 1000` | **fixed** — full `i16` range, plus a pinned regression case |
| 2 | deblocking bypasses `has_sse2()` | **fixed** — all four dispatchers gated |
| 3 | two misaligned raw derefs | **fixed** — `from_ne_bytes`, as the decoder twins already did |
| 5 | stack overflow at `width >= 19` | **fixed** — the deleted precondition is an assert again |
| 6 | AVX2 reachable from safe code | **fixed** — the wrappers self-guard and fall back to SSE2 |
| 6a | odd `H` reads past the block | **fixed** — `const { assert!(H % 2 == 0) }` |
| 7 | deblocking writes past the scalar's reach | **fixed** — write-back narrowed; the threading question is settled below |
| 7a | undocumented stride precondition | **fixed** — documented and `debug_assert`ed, via a new `PlaneSamples::stride` |
| 8 | `has_sse2()` latches a stale answer | **fixed** — one latched feature word behind both entry points |
| 9 | `InitMcFunc`'s gate is dead code | **documented** — the behaviour stands; see below |
| 10 | kernels that are SIMD in name only | **fixed** — two ported for real from upstream's asm, eight renamed |
| 11 | ~250 duplicated lines | **fixed** — 341 fewer lines of code across the four files |
| 14 | coverage gaps | **fixed** — 17 new parity tests; every public kernel in `dct.rs`, `intra_pred.rs` and `sad.rs` now has one |

---

## 1. IDCT vertical pass runs in `i16` where the scalar runs in `i32`

> **Resolved — widened to `i32`.** `compute_idct_residuals_sse2`
> (`simd/x86_64/dct.rs`) sign-extends each row register with a new
> `widen_lo_i16_to_i32_sse2` (SSE2 has no `pmovsxwd`; the idiom is
> `_mm_srai_epi32(_mm_unpacklo_epi16(v, v), 16)`), runs the butterfly in `epi32`, and
> narrows back with `_mm_packs_epi32`. The saturation there is unreachable and so
> exact: `|s*| <= 32768` bounds `|32 + t1 ± t2| <= 114720`, hence `|result| <= 1792`
> after the `>> 6`.
>
> **One correction to the analysis above.** It frames this as "matching upstream's asm
> and matching the port's own scalar path are mutually exclusive". Upstream is not
> self-consistent here either: `IdctResAddPred_AArch64_neon`
> (`codec/decoder/core/arm64/block_add_aarch64_neon.S:70`) widens with `saddl`/`ssubl`
> and runs `COL_TRANSFORM_1_STEP` entirely on `.4s` lanes — 32-bit, agreeing with
> upstream's own C. So upstream answers this question one way on x86 and the other way
> on aarch64, and the choice was which of its two answers to reproduce. The port now
> reproduces the one that agrees with its own scalar everywhere, and the divergence
> from `SSE2_IDCT_4x4P` is recorded on the kernel.
>
> The comment at `encoder/decode_mb_aux.rs` that claimed `i32` "over the full
> coefficient range" was true only of the `_c` path; it now says so and says what
> changed.
>
> No conformance golden moved: the committed streams' coefficients stay inside the
> range where the two agreed.

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

> **Resolved.** `lcg_i16` generates the full `i16` range. Verified to be a real
> guard rather than a wider-but-still-blind sweep: with the 16-bit kernel temporarily
> restored, `test_idct_res_add_pred_parity`, `test_idct_t4_rec_parity` and the new
> `idct_vertical_pass_does_not_wrap_at_16_bits` all fail; with the widened kernel all
> six pass. That last test pins this section's own worked example —
> `rs[0] = rs[8] = 20000`, zero prediction, scalar 255 against the old kernel's 0.

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

> **Resolved.** All four dispatchers in `common/deblocking_common.rs` now read
> `#[cfg(target_arch = "x86_64")] if crate::simd::has_sse2() { … }`, matching
> `common/mc.rs:312` and the eleven sites in `encoder/decode_mb_aux.rs`. The
> `#[allow(unreachable_code)]` that admitted the problem is gone, and
> `deblock_*_scalar` is reachable on x86_64 again — so `OPENH264_NO_SIMD=1` now turns
> deblocking scalar along with everything else.

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

> **Resolved.** Both are `i64::from_ne_bytes(top)` / `i32::from_ne_bytes(top)`,
> copied from the decoder twins four lines away as suggested. There are now no
> reinterpreting raw dereferences left in the SIMD tree.

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

## 5. Stack-buffer overflow in `mc_hor_ver22_inner_sse2` for `width >= 19`

> **Resolved.** `mc_hor_ver22_inner_sse2` asserts `width <= 17` before the vertical
> pass, restoring the precondition this series deleted and turning the silent
> four-byte overwrite into the same clean panic the scalar twin gives. The comment
> records where the bound comes from (`int16_t iTmp[17 + 5]` against
> `for (j = 0; j < iWidth + 5; j++)`) and which caller sits at the limit
> (`encoder/md.rs:1529`, `kiW + 1 = 17`).

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

> **Resolved — the feature test lives at the table, where the decision is made once.**
>
> ```rust
> #[cfg(target_arch = "x86_64")]
> if (uiCpuFlag & WELS_CPU_AVX2) != 0 && crate::simd::has_avx2() {
>     sdf.pfSampleSad[BLOCK_16x16] = Some(|a, b| sample_sad_16x16_avx2(a, b));
>     sdf.pfSampleSad[BLOCK_16x8]  = Some(|a, b| sample_sad_16x8_avx2(a, b));
> }
> ```
>
> Two conditions, and they are different questions. `uiCpuFlag` is the host's policy —
> a caller may restrict it, and `svc_mode_decision.rs:2371` passes `0`. `has_avx2()` is
> the hardware fact. The flag alone is not enough to install a `#[target_feature]`
> kernel, since nothing stops a caller passing a word it made up.
>
> The wrappers stay safe `fn` with the `unsafe` block inside and no branch of their own.
> What closes this section's actual complaint — "callable from entirely safe code" — is
> **`pub(crate)`**: the module boundary keeps the caller set to the one site that checks.
> That is weaker than a type-level proof and it is the right trade here. The alternatives
> both cost more than they are worth: a feature test inside the wrapper is a branch on
> every candidate the mode-decision loop scores, and an `unsafe fn` cannot be named at
> the call site at all, because `encoder/sample.rs` is `#![forbid(unsafe_code)]`.
>
> `init_sample_sad_installs_avx2_only_where_the_cpu_has_it` pins the table's behaviour
> through `WelsInitSampleSadFunc` itself. Be precise about what it covers: the **flag**
> half is pinned on every host — mutation-checked, dropping the `uiCpuFlag` test fails it
> here — but the **hardware** half can only fail on a machine without AVX2, since on one
> with it both spellings install the same kernel. The test says so in its own doc
> comment rather than leaving a green run on an AVX2 host to be read as more than it is.

---

## 7. Vertical deblocking rewrites samples the scalar never touches

> **Resolved, and the open question is settled: there is no race.** The chain, so
> nobody has to re-derive it:
>
> 1. `encoder_ext.rs:1205` — if the loop filter is on (`idc == 0`) and threading is on
>    (`iMultipleThreadIdc != 1`), the idc is rewritten to 2. Its own comment gives the
>    reason: "disable it on slice boundaries, since that is not allowed with
>    multithreading."
> 2. `deblocking.rs:1221` — `iLoopFilterDisableIdc == 2` is the per-slice walker, and
>    it sets `uiFilterIdc = 1`.
> 3. `DeblockingMbAvcbase` — with `uiFilterIdc == 1` the left-edge flag is
>    `bLeftBsValid[1] = iMbX > 0 && cur.uiSliceIdc == map[kiMbXY - 1]`. The left MB edge
>    is filtered **only when the left neighbour is in the same slice**.
> 4. Same slice means the same worker (`slice_multi_threading.rs:1193`, `:1647` deblock
>    the slice they just coded), so the two macroblocks are filtered sequentially.
>
> Columns −4 and −3 are the previous macroblock's only at `iEdge == 0`, and that edge
> is exactly the one gated. So the over-write was never concurrent.
>
> **The architecture-independent half is fixed anyway.** All four vertical arms now
> write back only the columns the filter can modify — `−2..=+1` for luma lt4,
> `−3..=+2` for eq4, `−1..=0` for both chroma kernels — instead of storing the whole
> read span. The SSE2 write contract is now the scalar's write contract, so the
> hazard is gone rather than argued about. Output is unchanged: the existing parity
> tests compare a −16..32 window around the anchor and all four still pass.
>
> One thing not done: a test that pins the *write reach* directly rather than the
> values. That needs a `PlaneSamples` implementation that records every `set`, and
> `RefSamples` carries an associated `Row` type and a `row_blocks` walk that make such
> a stub disproportionate to what it would catch now that the reach is narrowed.

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

> **Resolved.** `PlaneSamples` gained a `stride()` method (two implementors,
> `PlaneCursorMut` and `RecCursor`, both already had an inherent one), so the
> precondition is checkable rather than merely statable. All four `_sse2` dispatchers
> carry a `# Preconditions` section and a `debug_assert_eq!` on the cross-line step in
> each direction arm — both planes for the chroma pair.

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

> **Resolved.** `simd/mod.rs` now holds one `AtomicU32` feature word behind both
> entry points. `detect_cpu_features()` computes it once (bit 31, which no `WELS_CPU_*`
> flag uses, marks it computed, so a genuine all-scalar `0` is distinguishable from
> "not asked yet"), and `has_sse2()` / the new `has_avx2()` are one-line reads of it.
> The `SSE2_CACHED` half-latch is gone.
>
> The trade is that `OPENH264_NO_SIMD` becomes a process-start switch rather than a
> per-call one, which is what it already was in practice: `grep` finds three hits, all
> inside `simd/mod.rs`, and the tables a decoder builds at init never rebuild.

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

> **Documented, not fixed — and the difference matters.** The finding is exactly
> right and both halves of it stand: `grep` for the six `SMcFunc` field names still
> finds no read outside `common/mc.rs`, so a host that restricts `uiCpuFlag` still gets
> SSE2 motion compensation, and `init_mc_func_cpu_flags` still asserts a gate with no
> runtime effect.
>
> Closing it is the altitude change this section describes — routing MC back through
> the table, or threading the context's flag into the eighteen direct call sites — and
> that was deliberately left out of scope. What changed is that neither fact is
> discoverable only by grepping now: `InitMcFunc` carries a `# The table it fills is
> not this port's MC dispatch path` section naming the live mechanism and both
> consequences, and the test says in its own doc comment that passing it does not mean
> `uiCpuFlag` produces scalar MC.

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

> **Resolved — the two quant kernels are ported, not deleted.**
>
> First verified byte-identical rather than taken on the review's word:
> `dequant_ihadamard_4x4_sse2_impl` matched `encoder/decode_mb_aux.rs`'s scalar exactly,
> `hadamard_t4_dc_sse2` differed from `encoder/encode_mb_aux.rs`'s by one comment line,
> and both contained zero `_mm_*` calls. Both were removed with their dispatch-table
> overrides, and then written for real from upstream's asm —
> `WelsHadamardT4Dc_sse2` (`codec/encoder/core/x86/dct.asm:78`) and
> `WelsDequantIHadamard4x4_sse2` (`codec/encoder/core/x86/quant.asm:332`). They are
> installed in the tables again; the counts now are 33 and 13 intrinsics in the bodies,
> plus three shared helpers (`transpose4_epi32`, `transpose4_epi16_lo`,
> `ihadamard_butterfly_sse2`).
>
> Both are written from the *scalar's* semantics with the lanes laid out to match, not
> transliterated register-by-register from the asm, and both use the same shape: put one
> line per lane so the first pass is lane-wise, transpose once, second pass lane-wise.
> For the ihadamard that makes the two passes the **identical** butterfly — the row pass
> over `res[i..i+4]` and the column pass over `res[i], res[i+4], res[i+8], res[i+12]`
> have the same tap structure — so it is written once.
>
> Two details worth recording. `hadamard_t4_dc`'s output store is `_mm_packs_epi32`,
> whose signed saturation *is* the scalar's `.clamp(-32768, 32767) as i16`. And the
> ihadamard's multiply by `mf` goes at the **end**, where the scalar has it, rather than
> at the start where the asm has it: both are correct, since the transform is linear and
> every operation is wrapping `i16`, so `mf * (a ± b) ≡ mf * a ± mf * b (mod 2^16)`.
>
> **The tests came first**, since the absence of any was how a scalar copy sat in the
> dispatch tables unnoticed. Four of them, all over the full `i16` range because the
> ihadamard's wrapping is observable output and `hadamard_t4_dc`'s clamp is only
> reachable from large inputs: a 2000-case random sweep each, an exhaustive 65536-case
> sweep of every `i16::MIN`/`i16::MAX` assignment across the sixteen DC positions (with
> an assertion that it really does reach the clamp), and a wrapping check across the six
> dequant table rows plus `0`, `1`, `0x7FFF`, `0x8000` and `u16::MAX`.
>
> The eight intra-pred predictors named `_sse2` with no intrinsic are **renamed**, not
> deleted — they are not redundant clones but word-wide rewrites (a `u64`/`u32` load
> and store where the scalar does `copy_from_slice`/`fill`). The module header now
> states the convention: `_sse2` in this file means the body contains intrinsics.
>
> The eight intra-pred predictors named `_sse2` with no intrinsic are **renamed**, not
> deleted — they are not redundant clones but word-wide rewrites (a `u64`/`u32` load
> and store where the scalar does `copy_from_slice`/`fill`). The module header now
> states the convention: `_sse2` in this file means the body contains intrinsics.

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

> **Resolved — 341 fewer lines of code** (non-test, non-comment) across the four
> files: `simd/x86_64/mc.rs` −230, `simd/x86_64/intra_pred.rs` −101,
> `simd/x86_64/quant.rs` −57, `common/mc.rs` +47.
>
> **`mc.rs`.** Confirmed first that a single body is even valid: normalising the leaf
> suffix, twelve of the fifteen composites (01, 03, 10–13, 21, 23, 30–33) are
> character-for-character identical between `_c` and `_sse2`, and exactly the three
> real filters (02, 20, 22) differ. So `common/mc.rs` gained an `McLeaves` trait —
> `hor`/`ver`/`cen`/`avg` — with the twelve composites written once against it and
> `mc_luma_with::<L, S>` as the fan-out. `ScalarLeaves` lives there, `Sse2Leaves` in
> the SIMD module, and `mc_luma_c` / `mc_luma_sse2` are one line each. The twelve `_c`
> aliases were dropped after checking that nothing named them.
>
> As this section warns, that trades away the ability to catch a *structural* mistake
> by comparing the two spellings. What it buys is that they can no longer drift — the
> failure the parity tests could not see either, since they agreed with whichever copy
> they pointed at. The leaves stay individually tested and `test_mc_luma_parity` still
> compares the two instantiations across all sixteen positions.
>
> `scratch()`'s zero-init is gone from the SIMD side with the copies; the scalar side
> keeps it, since removing it needs `MaybeUninit` and `common/mc.rs` is
> `#![forbid(unsafe_code)]`.
>
> **`intra_pred.rs`.** The seven `enc_`/`dec_` pairs differed only at the store — the
> neighbour reads were already `RefSamples::at`/`row_n` on both sides. A `PredOut`
> trait (`Packed<W>` for the encoder's candidate buffer, `PlaneCursorMut` for the
> decoder) plus shared halves — `i16x16_dc_mean_sse2`, `chroma_dc_rows`,
> `i16x16_plane_coeffs`/`chroma_plane_coeffs` and their fills, `pred_v`/`pred_h`/
> `fill_rows` — leave fourteen entry points of three lines each. The stores became
> `copy_from_slice` over fixed-size arrays, which is the same instruction and several
> fewer `unsafe` blocks. The three DC variants (DC, DC-top, DC-NA) collapsed into one
> body as well.

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


## 14. Coverage gaps in `dct.rs`

> **Resolved — 17 new tests, and the gap was worse than "no test".**
>
> The nine `dct.rs` entry points did have coverage of a kind: the
> `*_matches_the_plane_cursor_form` tests in `encoder/decode_mb_aux.rs`. But on x86_64
> *both* sides of those dispatch into `simd/x86_64/dct.rs`, so they pin the
> `RecCursor`-vs-`PlaneCursorMut` equivalence and not SSE2 against scalar. The nine new
> tests run each kernel against `idct_t4_rec_c` / `idct_t4_rec_in_place_c` /
> `idct_rec_i16x16_dc_c`, and reference the multi-block forms against the scalar
> applied *per block* at the sub-offsets rather than against this file's own
> four-block loop — so the hand-written `off`/`advance` arithmetic is under test and
> not a shared assumption. Whole allocations are compared, padding included.
> Mutation-checked: transposing `dy * pred_stride + dx` in
> `idct_four_t4_rec_to_view_sse2_impl` fails exactly that test and nothing else.
>
> `intra_pred.rs` was 3 tests for 30 kernels because the three covered all twelve
> `enc_*` predictors and none of the thirteen `dec_*` ones — the in-place
> reconstructors the decoder installs at `decoder_core.rs:1817..1830`, which are the
> shape where an off-by-one row or column hides. Three new tests cover all thirteen
> against `decoder::get_intra_predictor`, comparing whole allocations.
>
> `sad.rs`'s three tests did reach all sixteen public kernels, but each at one anchor
> over one input pattern. Four new tests sweep every kernel over four anchors (one per
> residue mod 8) and five distributions, including the all-`0xFF`/all-`0x00` pair and a
> near-identical pair that sit at the ends of the accumulator's range. The AVX2 sweep
> announces the skip on a host without AVX2 instead of reporting "ok" having executed
> nothing.

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
