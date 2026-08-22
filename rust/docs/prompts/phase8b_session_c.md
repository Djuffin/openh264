# Phase 8b — Session C: downsample, denoise, the decoder's pool-growth arm, and the phase close

You are one session in a long, careful port of the openh264 codec from C/C++ to Rust.
The Rust crate is at `rust/crates/openh264-rs/`; the C++ reference is the rest of the
repo under `codec/`. The crate is a **byte-identical drop-in replacement** for
`libopenh264`: the same C ABI, the same output bytes, the same error codes. Your job is
to close **Phase 8b**, whose goal is *feature and correctness parity* — making the port
do things the C++ does that the port currently does not.

Work top to bottom. Each step says its Goal, the Facts you need, what to Do, and the
Accept bar that ends it. If the tree disagrees with a number in this prompt, the tree is
right — re-grep and note it in your report. Commit with messages `T8b.C1`, `T8b.C2`, …
Record findings in `rust/docs/phase8b_findings.md`, numbered from **F97**. Leave
breadcrumbs in `rust/docs/safety_refactor_log.md` as you go.

---

## The instrument you are moving

Upstream ships its own test suite, `test/api/` (199 gtest cases). A tool links those
tests against the Rust cdylib and against `libopenh264.a`, runs both, and diffs:

```bash
rust/tools/abi_harness/gtest_stretch.sh --check          # the gate, one seed
rust/tools/abi_harness/gtest_stretch.sh --seeds=1..10     # the finder, many seeds
```

The suite seeds `rand()` from the clock, so a run is only reproducible at a fixed seed;
`--check` pins `SEED=20260822`. It reads an allowlist of known failures,
`rust/tools/abi_harness/gtest_known_failures.txt` (format: `full test name | owner |
reason`), and is **red** if any test fails that is not on the list, **or** if any listed
test passes (a stale row). It is wired into `gates.sh exit` as gate 6c.

At the start of this session the tally is **173 / 199 passing, 26 rows allowlisted**:

- **18 rows owned by this session** (`8b.C`) — the downsample/denoise family, listed below.
- **7 rows owned by Phase 10** (screen content) — leave them.
- **1 row, D-poc-1** (`DecoderOutputTest.CompareOutput/39`) — a deliberate, permanent
  divergence (the port breaks a picture-order tie the standard's way; upstream does not).
  Leave it; it stays on the list forever.

**Closing this session means the 18 `8b.C` rows pass and leave the list, so the exit
allowlist is exactly the 7 Phase 10 rows plus D-poc-1.**

The 18 rows (`grep 8b.C rust/tools/abi_harness/gtest_known_failures.txt`):

```
EncodeFile/EncoderOutputTest.CompareOutput/4        denoise on, 1 layer
EncodeFile/EncoderOutputTest.CompareOutput/5        2 spatial layers (dual golden hash)
EncodeFile/EncoderOutputTest.CompareOutput/7        4 spatial layers, 1280x720 (dual hash)
EncodeDecodeTestAPI.SimulcastAVC                     multi-layer encode
EncodeDecodeTestAPI.SimulcastAVCDiffFps              multi-layer
EncodeDecodeTestAPI.SimulcastAVC_SPS_PPS_LISTING     multi-layer
EncodeDecodeTestAPI.SimulcastSVC                     multi-layer
EncodeDecodeTestAPI.DiffSlicingInDlayer              multi-layer
EncodeDecodeTestAPI.DiffSlicingInDlayerMixed         multi-layer
EncodeDecodeTestAPI.AVCSVCExtensionCheck             multi-layer
EncodeDecodeTestAPI.SetOptionEncParamExt             multi-layer
EncodeDecodeTestAPI.SetOptionEncParamBase            multi-layer
EncodeDecodeTestAPI.Engine_SVC_Switch_I              multi-layer
EncodeDecodeTestAPI.Engine_SVC_Switch_P              multi-layer
EncodeDecodeTestAPIBase/EncodeDecodeTestAPI.GetOptionTid_SVC_L1_NOLOSS/0..2   multi-layer
DecodeParseAPI.ParseOnly_General                     3 spatial layers, then parse
```

Every one of these needs the encoder to produce **more than one spatial layer**, and a
multi-layer encode **downsamples** the source into each lower layer. Downsample is the
single missing piece behind all 18 (`ParseOnly_General`'s parse half already works —
only its 3-layer encode fails). Row 4 is the exception: it needs **denoise**, not
downsample, and no layers.

---

## The one risk that can sink this session: which downsampler the reference runs

Read this before writing any kernel.

The reference library the tests link — `libopenh264.a` — is built with assembly
enabled. Confirm:

```bash
nm -gU libopenh264.a | grep -c AArch64_neon        # ~160
nm -gU libopenh264.a | grep -i Downsampler | grep neon
```

On this arm64 host, `libopenh264` dispatches at runtime to its **NEON** downsample
kernels (`DyadicBilinearDownsampler_AArch64_neon` and friends). The port would naturally
translate the **`_c`** kernels (`DyadicBilinearDownsampler_c` and friends). **These two
are not guaranteed to produce the same bytes**, and upstream says so itself: in
`test/api/encoder_test.cpp:163` and `:174`, the `EncoderOutputTest` rows 5 and 7 carry
*two* golden hashes each, with the comment:

> Allow for different output depending on whether averaging is done vertically or
> horizontally first when downsampling.

For every kernel the port has matched so far (SAD, DCT, intra prediction), NEON and `_c`
are bit-identical — which is why the port has been byte-exact against this same NEON
library for the whole project. But **no encode the port has ever run downsamples**: every
sweep and bench uses one spatial layer, where source and destination are the same size
and the code copies rather than scales. Downsample is the *first* kernel family where the
port and the reference can execute genuinely different code.

So a faithful `_c` port may or may not match the referee. **Step 0 settles this by
measurement before you write a kernel.** Do not skip it and do not guess.

---

## Hard rules

1. **Gating (D-gate-3): fast between commits, heavy once at the end.**
   - Per commit: `rust/tools/gates.sh commit` (~2–3 min: builds all targets, runs
     `cargo test` debug + release which already carry conformance + the malformed
     corpus, the unsafe ratchet, the duplicate census) **plus** the in-tree test the
     commit adds.
   - Per session-close (step 7): the fast set only — `gates.sh commit`,
     `gtest_stretch.sh --check`, `abi_exports.sh`, `abi_harness/run.sh`, `compare_all.sh`.
   - **The heavy battery runs once, at the phase close (step 8):** `gates.sh exit`
     unscoped — the diffharness sweeps in both profiles, both benches, Miri over the
     whole library, and a single 7-pair performance span. **Do not run sweeps, Miri, or
     benches mid-session.** A byte divergence found at the close is bisected across your
     commits, which stay small.

2. **The reference is the test.** Every feature you add lands with a referee that reads
   the C++'s output for the same input, and you **run it red first** (fails without your
   change) then green. No feature is "done" on a green build alone.

3. **New code is safe Rust.** The crate is converging on `#![forbid(unsafe_code)]`. Write
   safe Rust. If a raw pointer is forced on you by a raw-pointer *caller* you cannot
   change this session, tag the site exactly:
   ```rust
   // unsafe-cat: port-raw(Phase 9)
   #[allow(unsafe_code)]
   ```
   `rust/tools/unsafe_ratchet.sh check` must stay green per commit; if you must raise a
   count deliberately, re-baseline it (`unsafe_ratchet.sh generate`) in the *same* commit
   and say why in the message. **The processing kernels (downsample, denoise) are pure
   arithmetic over slices — they must be 100% safe, no `unsafe`, no raw pointers.** The
   pool-growth work in step 3 is also safe code (see there).

4. **Unfinished work refuses loudly.** If you cannot finish something, it must return an
   explicit error code that reaches the API (never silently succeed), with a test that
   asks for the feature and asserts the error, and an allowlist row naming the next
   owner. A stub that returns success is a bug (this is why row 4's denoise and the
   downsample branch currently mislead — they returned "not supported" but the caller
   dropped it; you are fixing exactly that class).

---

## Where the code lives, and the shapes you will touch

### The port's video-processing dispatch

`rust/crates/openh264-rs/src/processing/mod.rs` holds `SWelsVpContext`, one field per
implemented processing method:

```rust
pub struct SWelsVpContext {
    pub sVaaCalc: CVAACalculation,
    pub sComplexityAnalysis: CComplexityAnalysis,
    pub sAdaptiveQuant: CAdaptiveQuantization,
    pub sBackgroundDetection: CBackgroundDetection,
    pub sSceneChangeDetection: CSceneChangeDetection,
}
```

Each of those is a small struct with `Set` / `Process` methods, ported from a C++
plugin. The encoder owns one `SWelsVpContext` as `m_vp` and calls e.g.
`self.m_vp.sVaaCalc.Process(...)` (`wels_preprocess.rs:1736`). **Downsample and denoise
are two more methods in exactly this shape** — add `sDownsample` and `sDenoise` fields (or
free functions in a `processing::downsample` / `processing::denoise` module; match the
existing plugins' style) and call them from the two sites named below. `mod.rs:29–39`
lists them today under a *"Not translated"* table — that table shrinks as you go.

### The two call sites in the encoder

Both are in `rust/crates/openh264-rs/src/encoder/wels_preprocess.rs`:

- **`DownsamplePadding`** (`:1596`) builds a source `SPixMap` and a destination `SPixMap`
  from two pictures, and at `:1642` has this dead branch:
  ```rust
  if iSrcWidth != iShrinkWidth || iSrcHeight != iShrinkHeight {
      // METHOD_DOWNSAMPLE: untranslated ...
      iRet = crate::processing::vaacalc::RET_NOTSUPPORTED;   // <-- replace this
  } else {
      WelsMoveMemory_c(...);   // the same-size copy, already correct
  }
  ```
  Replace the `RET_NOTSUPPORTED` line with a real call into your downsample dispatch,
  passing the two `SPixMap`s. `SPixMap` (`wels_preprocess.rs:265`) carries
  `pPixel: [*mut u8; 3]`, `iStride: [i32; 3]`, and `sRect { iRectWidth, iRectHeight }`
  — the pointers come from `SPicture::planes()` (already resolved above the branch). The
  kernel itself must take **slices**, not those raw pointers: convert at the boundary
  (you own both pictures; build `&mut [u8]`/`&[u8]` from the plane + stride + height).
- **`BilateralDenoising`** (`:1590`) is an empty body behind `bEnableDenoise`:
  ```rust
  pub unsafe fn BilateralDenoising(&mut self, _pSrc: SrcPicRef, ...) {
      // METHOD_DENOISE: untranslated — no plugin runs
  }
  ```
  Fill it: resolve the source picture's three planes and run the denoise kernels in place.

`SingleLayerPreprocess` (`:1184`) is what calls `DownsamplePadding` per dependency layer
(`:1240`, `:1327`); you do not need to change it — once `DownsamplePadding` scales
instead of refusing, the multi-layer path works.

### The C++ you are translating

**Downsample dispatch** — `codec/processing/src/downsample/downsample.cpp`,
`CDownsampling::Process` (`:144`). Given source W×H and destination w×h it picks a kernel
by the ratio (all integer):

- `RET_INVALIDPARAM` if `srcW <= dstW || srcH <= dstH`.
- The port's pictures are always ≤ 1920×1088 luma after the `>>1`, so you take the
  **first** branch of `Process` (`m_bNoSampleBuffer` is the single-pass path; the
  multi-pass `else` with `m_pSampleBuffer` is only for sources wider than
  `MAX_SAMPLE_WIDTH=1920` — confirm your test streams stay under it and, if any does not,
  port the multi-pass loop too). Within that branch:
  - `srcW>>1 == dstW && srcH>>1 == dstH` → `DownsampleHalfAverage` (a wrapper; see
    `downsample.cpp:279`) on Y, U, V.
  - `srcW>>2 == dstW && srcH>>2 == dstH` → `pfQuarterDownsampler` =
    `DyadicBilinearQuarterDownsampler_c`.
  - `srcW/3 == dstW && srcH/3 == dstH` → `pfOneThirdDownsampler` =
    `DyadicBilinearOneThirdDownsampler_c`.
  - otherwise → `pfGeneralRatioLuma` = `GeneralBilinearFastDownsampler_c` on Y,
    `pfGeneralRatioChroma` = `GeneralBilinearAccurateDownsampler_c` on U and V.
  Chroma dims are the luma dims `>>1`. Read the exact argument order in the source; it
  differs between the dyadic kernels (`pDst, dstStride, pSrc, srcStride, srcW, srcH`) and
  the general kernels (`pDst, dstStride, dstW, dstH, pSrc, srcStride, srcW, srcH`).

**Downsample kernels** — `codec/processing/src/downsample/downsamplefuncs.cpp`. The
dyadic one is a 2×2 box average:
```c
void DyadicBilinearDownsampler_c (uint8_t* pDst, int32_t kiDstStride,
                                  uint8_t* pSrc, int32_t kiSrcStride,
                                  int32_t kiSrcWidth, int32_t kiSrcHeight) {
  for (j < srcH>>1) for (i < srcW>>1) {
    row1 = (src[2i] + src[2i+1] + 1) >> 1;
    row2 = (src[2i+srcStride] + src[2i+srcStride+1] + 1) >> 1;
    dst[i] = (row1 + row2 + 1) >> 1;
  }
}
```
Quarter (`:71`) and OneThird (`:95`) are the same idea at 4:1 and 3:1; General
Fast/Accurate (`:118`, `:187`) use a fixed-point `16.16`/`16.15` bilinear scale with
`WELS_ROUND`. Translate each one faithfully — these are the bytes the goldens check.

**Denoise** — `codec/processing/src/denoise/`. `CDenoiser::Process` (`denoise.cpp:66`)
guards on `m_uiType`'s component bits and runs, in place:
- `BilateralDenoiseLuma(pY, W, H, strideY)` (`:92`): for each row/col inside a border of
  `m_uiSpaceRadius = DENOISE_GRAY_RADIUS = 1`, run `BilateralLumaFilter8_c` in 8-pixel
  steps (`denoise_filter.cpp:41`) and `Gauss3x3Filter` (`:114`) on the tail.
- `WaverageDenoiseChroma(pU/pV, W>>1, H>>1, strideUV)` (`:107`): same shape with
  `UV_WINDOWS_RADIUS = 2` and `WaverageChromaFilter8_c` (`:89`).
Constants: `TAIL_OF_LINE8 = 7`. The default `m_uiType` enables all three components; read
the encoder's denoise setup to confirm which components row 4 exercises.

### The gtest rows' parameters (for building targeted referees)

`test/api/encoder_test.cpp:140`, `kFileParamArray`, indexed by the `/N` in the row name:
- `/4`: `CiscoVT2people_320x192_12fps.yuv`, `CAMERA_VIDEO_REAL_TIME, 320, 192, 12fps,
  SM_SINGLE_SLICE, bDenoise=true, layers=1` — **denoise, no downsample**.
- `/5`: same input, `layers=2`, dual golden hash — **downsample 2:1**.
- `/7`: `Cisco_Absolute_Power_1280x720_30fps.yuv`, `layers=4` — **downsample, 4 layers**.

---

## The diffharness (your byte-parity referee for the encoder)

`rust/tools/diffharness/` has two encoders driven by the same config: `cxx_enc` (links
`libopenh264.a` — the NEON reference) and `rust_enc` (links the port). `sweep.sh` runs
preset batteries; `check` runs one config and diffs the coded bytes. The presets:
`st mt qp def sl ltr` and `ps` (`sweep_ps`, five `eSpsPpsIdStrategy` values — session B
added this but did not run it; note it is registered as **`ps`** even though earlier
notes called it `sp`; reconcile to one name in step 5).

**There is no multi-layer preset yet.** You will add one (call it `dl`, dependency
layers) that drives `iSpatialLayerNum` 2 and 4 with denoise on/off — this is the preset
that actually exercises downsample. It runs at the phase close (step 8), but you build a
*single targeted config* by hand per step to verify as you go (D-gate-3: no full sweep
mid-session).

`cxx_enc`/`rust_enc` config flags include `iSpatialLayerNum` and `bEnableDenoise`
(`cxx_enc.cpp:85,123`; `rust_enc/main.rs:69,84`). If step 0 decides the reference must
run `_c` downsamplers for a fair comparison, both drivers can force the CPU dispatch to
scalar by setting the environment the library reads (`WELSCPUFLAG=0`) or by linking a
`USE_ASM=No` build of the reference — decide in step 0 and document it at the preset.

---

## The decoder's pool-growth arm (F80 / F87) — a separate, independent task

This has nothing to do with downsample; it is a decoder gap you also close this session.

**What is wrong:** when a stream changes its *reference-frame count* without changing
resolution, the reference decoder resizes its picture pool in place and keeps decoding;
the port returns `dsOutOfMemory` and stops. Reproduce:

```bash
rust/tools/ecref/ecref res/../crates/openh264-rs/tests/data/f80/num_ref_change_320x192.264 99999999 --frames
# reference: 48 320x192 <hash>   (decodes all 48 frames)
```
The asset (24 frames at `iNumRefFrame=1`, then 24 at 4, same 320×192) was built with the
reference encoder by `rust/tools/make_numref_asset.cpp`. The reference logs, at the
switch: `"memory re-alloc for no resolution change (size = 320 * 192), ref list size
change from 3 to 6"`. The port emits nothing for the second half.

**The C++ this needs** — `codec/decoder/core/src/decoder.cpp`, `WelsRequestMem`
(`:464–545`) has three arms; the port (`decoder_core.rs`, `SyncPictureResolutionExt`
`:944`) has two. The missing one is:
```c
// same resolution, but capacity != requested queue size:
if (pCtx->pPicBuff->iCapacity < iPicQueueSize)
    IncreasePicBuff(pCtx, &pCtx->pPicBuff, oldCap, W, H, iPicQueueSize);
else
    DecreasePicBuff(pCtx, &pCtx->pPicBuff, oldCap, W, H, iPicQueueSize);
```
- `IncreasePicBuff` (`decoder.cpp:107–163`): allocate a bigger pool, allocate the new
  tail pictures, **copy the old picture handles into the front**, keep `iCurrentIdx`,
  reset per-picture ref flags, free the old container.
- `DecreasePicBuff` (`decoder.cpp:170–235`): allocate a smaller pool; if the DPB's
  "previous decoded picture" sits beyond the new size, move it to slot 0; copy the kept
  handles; **clear every `pRefPic[list][j]` across the new pool** (oss-fuzz 14423); free
  the dropped pictures.

**The port's pool** — `rust/crates/openh264-rs/src/decoder/pic_queue.rs`, `PicPool`
(`:121`) wraps `safe::Pool<PicSlot>` where `PicSlot = Option<Box<SPicture>>`. `safe::Pool`
(`rust/crates/openh264-rs/src/safe/pool.rs`) documents itself as **"never grows or
shrinks"** (`:97`) and each handle `Id` carries a debug-only *generation* counter
(`:58`) so a stale handle is caught in tests. **This is the one `src/safe/` invariant you
change this session**, in safe code:
- Add `Pool::grow(&mut self, extra: Vec<T>)` and `Pool::shrink_to(&mut self, keep:
  &[Id]) -> Vec<T>` (or the exact shape `Increase`/`Decrease` need — read them first).
- Decide the generation contract across grow/shrink and **test it** (a grown slot starts
  at generation 0; a handle to a dropped slot must never compare equal to a handle to a
  slot that later reuses its index). This is the same class of test `safe/pool.rs`
  already has for `replace`; add the grow/shrink analogue.

**Referee:** move the asset into `res/` **in the same commit** as the fix (the test
`decoder_reachability_sweep.rs` globs `res/` and would go red on an un-decodable asset
until then), and add an in-tree parity test asserting frames, dimensions, per-call error
codes and `iBufferStatus` against the reference — build the goldens with `ecref`. Run it
red first (it fails today with `dsOutOfMemory`), green after.

---

## The two concealment divergences (F96, F88) — narrowed, waiting for a fix

These are real output divergences on damaged streams under specific error-concealment
modes. Both were localized by earlier work; you decide whether the fix is small enough to
land here (one enumerated behaviour change with a covering test) or gets filed with its
repro for Phase 9.

**F96 — `ERROR_CON_DISABLE` on `res/Error_I_P.264`.** With concealment *disabled*, the
port reports `dsBitstreamError` (0x4) at decode calls 6 and 16 of 17 where the reference
reports `dsRefLost` (0x2). Referee: `rust/tools/ecref/build.sh` builds **`ecref_rs`** —
the same program linked against the port's cdylib — so you can run the identical query on
both:
```bash
rust/tools/ecref/ecref    res/Error_I_P.264 99999999 --ec=0
rust/tools/ecref/ecref_rs res/Error_I_P.264 99999999 --ec=0
```
The cause is inside `InitRefPicList` (`decoder_core.rs:4220`) or one of `WelsInitRefList`
/ `WelsReorderRefList` / `WelsReorderRefList2` beneath it: the reference's
`InitRefPicList` returns `ERR_INFO_REFERENCE_PIC_LOST` and, under disabled concealment,
returns from the slice early; the port returns `ERR_NONE` and then fails the slice
decode. Already eliminated as the cause: `WelsCheckAndRecoverForFutureDecoding` (its body
is skipped under DISABLE) and `WelsReorderRefList` (its failures set `iErrorCode`, but the
observed code path does not). One trace line the port does not emit —
`decoder_core.cpp:2713`, *"reference picture ... is lost during transmission!"* — names
the arm; if you fix F96, port that line **with** the fix (it is a behaviour change on the
trace surface and belongs with what it explains, not before).

**F88 — `ERROR_CON_SLICE_COPY`, intermittent.** In the full gtest suite,
`EncodeDecodeTestAPI.SetOptionECIDC_SpecificFrameChange` fails on the Rust link at
`decoder_ec_test.cpp:302` — `EXPECT_EQ(dstBufInfo_.iBufferStatus, 1)` — on **1 seed in
10** (the C++ link passes all 10). It reproduces at `gtest_stretch.sh --seeds=5..5`
(read `rust/tools/abi_harness/out/gtest/rust_5.log`). It is a *concealment* divergence: on
a frame decoded after an earlier frame was dropped, the reference conceals and emits a
frame (`iBufferStatus=1`) where the port emits nothing (`0`). It needs the encoder in the
loop, so it lives in the gtest binary, not a `res/` asset. Time-box investigation to 2
hours; instrument `DecodeFrameConstruction`'s output decision on that access unit and
compare the two links, or file it with the seed and hand it to Phase 9.

**F93 — parse-only on `Error_I_P.264` under DISABLE.** This is F96 seen through the
parse-only path (which forces `ERROR_CON_DISABLE`), plus extra divergence parse-only adds
on top. Re-measure it *after* F96 with `ecref --parse-only --ec=0` vs `ecref_rs`; if F96's
fix resolves it, close both; if residue remains, file it.

---

## Steps

### Step 0 — Settle the downsampler question (do this first, write no kernel yet)
**Goal:** know, by measurement, whether the port's `_c` downsampler will match the
reference the tests link, so you know what "byte-identical" even means for this session.
**Facts:** `libopenh264.a` links NEON downsamplers (verified via `nm`); `_c` and NEON may
differ; upstream's rows 5/7 carry two goldens for exactly this reason.
**Do:** write a tiny throwaway harness that runs the reference's `_c` and NEON kernels
over identical input and diffs the output — either call `DyadicBilinearDownsampler_c` and
`DyadicBilinearDownsampler_AArch64_neon` directly (both are in `libopenh264.a`), or run
one 2-layer encode twice with `WELSCPUFLAG=0` and without and diff. Do the same for the
denoise filter (`BilateralLumaFilter8_c` vs its NEON sibling).
**Do:** decide and record, in this file's terms:
- If `_c == NEON` byte-for-byte on the ratios the tests use → port `_c`; parity against
  `cxx_enc` holds as-is. Keep the proof as a checked-in test or a logged measurement.
- If they differ **only** by the documented vertical/horizontal averaging order and the
  gtest rows already carry both hashes → port `_c`, confirm the port matches **one** of
  the two goldens, and make the diffharness reference run `_c` too (force `WELSCPUFLAG=0`
  in both drivers, or link a `USE_ASM=No` reference for the sweep); document the choice at
  the new preset and in `perf_baseline.md`.
- If they differ in a way the goldens do **not** cover → **stop and put it in your report
  as a blocking question for the steward.** Do not port a kernel that cannot match its
  referee. Steps 3 and 4 (pool growth, F96/F88) are independent and still proceed.
**Accept:** a checked-in proof of `_c`-vs-NEON identity (or the exact difference) for
downsample and denoise, and a one-line recorded decision.

### Step 1 — Denoise (closes row 4)
**Goal:** `EncoderOutputTest.CompareOutput/4` passes.
**Facts:** row 4 is denoise on, 1 layer — no downsample, so it is independent of step 0's
downsample outcome. C++ at `denoise.cpp` / `denoise_filter.cpp` (shapes above).
**Do:** port `BilateralLumaFilter8_c`, `WaverageChromaFilter8_c`, `Gauss3x3Filter`,
`BilateralDenoiseLuma`, `WaverageDenoiseChroma` and `Process` into a `processing` module,
all safe over slices; fill `BilateralDenoising` (`wels_preprocess.rs:1590`) to resolve the
source planes and run them in place.
**Do:** referee — a targeted `rust_enc`/`cxx_enc` pair with `bEnableDenoise` on; run red
first, green after. Then remove row 4 from the allowlist in the same commit.
**Accept:** row 4 passes and leaves the list; the denoise pair is byte-identical (against
the reference resolved in step 0); `gtest --check` count +1.

### Step 2 — Downsample (closes rows 5, 7, and the multi-layer cluster)
**Goal:** the 2- and 4-layer encodes match the reference, so rows 5, 7 and the
Simulcast/DiffSlicing/SVC-Switch/SetOptionEncParam/GetOptionTid/ParseOnly_General rows
pass.
**Facts:** dispatch and kernels above; the port call site is `DownsamplePadding:1642`.
**Do:** port `DownsampleHalfAverage`, the three dyadic kernels and the two general kernels
(per step 0's decision), and `Process`'s ratio dispatch, into `processing`, safe over
slices; replace the `RET_NOTSUPPORTED` line with a real call. Add a `dl` diffharness
preset (`iSpatialLayerNum` 2/4 × denoise on/off) but **do not run the full preset now** —
build one targeted config per layer count by hand and diff.
**Do:** run each targeted pair red first (they diverge or error today), green after;
remove the rows they fix from the allowlist as each passes.
**Accept:** rows 5 and 7 and the multi-layer rows that only needed downsample leave the
list; each targeted pair is byte-identical; any row that needs *more* than downsample is
named with why and left owned. `gtest --check` climbs toward 191.

### Step 3 — The decoder's pool-growth arm (closes F80 / F87)
**Goal:** `num_ref_change_320x192.264` decodes all 48 frames matching the reference.
**Facts:** `WelsRequestMem`'s third arm; `Increase`/`DecreasePicBuff`; `safe::Pool`'s
never-grow contract (all above).
**Do:** add `Pool::grow`/`shrink_to` with a tested generation contract; port
`IncreasePicBuff`/`DecreasePicBuff` into `pic_queue.rs`; wire the third arm into
`SyncPictureResolutionExt`. Move the asset into `res/` in the same commit. Add the in-tree
parity test (frames, dims, per-call codes, `iBufferStatus` from `ecref`); run it red
first, green after.
**Accept:** the asset decodes 48 frames byte-matching the reference;
`decoder_reachability_sweep.rs` accepts it; F80/F87 closed in `phase8b_findings.md`.

### Step 4 — The concealment divergences (F96, then F88, then F93)
**Goal:** close F96 if the fix is one enumerated change; progress F88 and F93.
**Facts:** F96 is in `InitRefPicList`, narrowed, `ecref_rs --ec=0` is the referee; F88 is
concealment under SLICE_COPY at seed 5; F93 is F96 via parse-only (all above).
**Do:** F96 first (port its trace line with the fix). If it is small and covered by a
test, land it; else file it with the `ecref_rs` repro. F88: reproduce at `--seeds=5..5`,
instrument the output decision, 2-hour box, fix-or-file. F93: re-measure after F96.
**Accept:** F96 closed with a covering test or filed with a named cause; F88 and F93
dispositioned (fixed or owned by name).

### Step 5 — Census and instrument tidy-up
**Goal:** the port-completeness census shows no open `8b.C` gaps; the preset is named
once.
**Facts:** `rust/tools/port_census.py --classify` lists what the reference has and the
port lacks; the downsample/denoise functions are its `8b.C` rows. The sweep preset is
`ps` in the tree but `sp` in older notes.
**Do:** re-run the census; the downsample/denoise functions move to `present`; regenerate
`rust/docs/phase8b_port_census.md`. Reconcile the preset name to one spelling in
`sweep.sh`, the charter, and the log. **If time permits**, fix `find_stub_bodies.py` so it
counts statements inside a leading `abi_guard!` / `panic_probe!` block (today it reads
every C-ABI thunk as a 0-statement stub because it sees one macro call — so its signal on
all 20 boundary slots is zero); this is a measurement change and takes its own commit.
**Accept:** census `8b.C` = 0 open; the preset has one name.

### Step 6 — If you run short, drop from the end
Drop order: the `find_stub_bodies.py` fix (→ Phase 9), then F93, then F88 (keep the seed
in the finding), then F96's trace line (keep the fix). **Never drop step 0, the
downsample/denoise referees, or step 3's test.**

### Step 7 — Session-close fast set (D-gate-3)
Run and record: `gates.sh commit`; `gtest_stretch.sh --check` (pinned seed);
`abi_exports.sh release`; `abi_harness/run.sh`; `ecref/compare_all.sh` and an `ecref_rs`
parity pass. Update `phase8b.md` §5's C row and write the log entry. **No sweeps, Miri,
benches, or span here.**

### Step 8 — The phase close (the only heavy run)
**Goal:** close Phase 8b with the full battery green and the reconciliation written.
**Do:** `gates.sh exit` **unscoped** — diffharness sweeps in both profiles *including your
new `dl` preset and `ps`*, both benches, Miri over the whole library and the differential
tests. Then the **one** 7-pair performance span with its 3-pair null, stated against the
project's +25%-median tripwire (it should not move — you added arithmetic kernels on a
path that only runs for multi-layer encodes).
**Do:** confirm the exit allowlist is **exactly** the 7 Phase 10 rows + D-poc-1, every
other row gone. Write the **Phase 8b CLOSED** reconciliation: the tally's arc across A/B/C,
the findings ledger, and what Phase 9 (safety endgame) and Phase 10 (screen content)
inherit. Check every exit condition in `phase8b.md` §4.
**Accept:** `gates.sh exit` OVERALL PASS; allowlist at 8 rows; the reconciliation and the
span table written.

---

## Do not touch

| Item | Why / who owns it |
|---|---|
| `DecoderOutputTest.CompareOutput/39` | D-poc-1 — a deliberate, permanent divergence |
| the 7 screen-content allowlist rows | Phase 10 |
| any `// unsafe-cat: port-raw(Phase 9)` signature (converting it to safe) | Phase 9 |
| the encoder threading seam, the context split, the `Send` verdict | Phase 9 |
| `SParserBsInfo`'s two-entity rename, `libc` removal, the crate-root deny flip | Phase 9 |
| mid-session sweeps / Miri / benches / perf spans | the phase close only (D-gate-3) |

---

## Report back (what the steward needs)

1. **Commits** — hash + one line each.
2. **Step 0's answer, in full** — the `_c`-vs-NEON measurement for downsample and
   denoise, and the decision you took. This is the session's pivot; do not summarize it
   away.
3. **The tally** — before/after, counted by the assertion site each row fails at (not by
   test name); the allowlist diff; confirm it ends at 8 rows (7 Phase 10 + D-poc-1).
4. **Per step** — what landed, the measured-red evidence for each referee, the anchors
   you touched.
5. **F80/F87, F96, F88, F93** — status of each (closed / filed with repro / owned).
6. **The phase-close heavy run** — the `gates.sh exit` verdict, the sweep/Miri/bench
   lines, and the span table (decode/encode median, min, max, rows over 5%).
7. **Gate lines** — session-close and phase-close; ratchet deltas with reasons for any
   rebaseline.
8. **Findings F97+**, and any fact in this prompt that did not survive contact with the
   tree (quote the prompt's wrong version and the tree's right one).
9. **The Phase 8b CLOSED reconciliation** and what Phases 9 and 10 inherit.
