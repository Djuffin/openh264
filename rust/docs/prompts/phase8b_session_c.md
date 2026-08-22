# Phase 8b — session C: downsample, denoise, F80's pool growth, and the phase close

*Written 2026-08-22 by the steward at `65042ac9` (session B's close). Execute top to
bottom; drop from the end. Every count was re-grepped at that commit — re-grep before
quoting (S24); if the tree disagrees, the tree wins and the disagreement goes in the
report. This session closes Phase 8b.*

## Context

Phase 8b makes the port *work* (D-prio-1). The instrument is upstream's `test/api`
against the Rust cdylib, seed-pinned: `gtest_stretch.sh --check` at `SEED=20260822`,
gate 6c of `gates.sh exit`, allowlist `tools/abi_harness/gtest_known_failures.txt` —
**173/199 at `65042ac9`, 26 rows, every row owned**: 18 `8b.C` (this session), 7 Phase 10,
1 D-poc-1 (permanent). Sessions A and B did the decoder option arms, `ForceCodingIDR`,
parse-only, and the five parameter-set strategies. This session does the last parity
family — **downsample and denoise** — plus **F80/F87** (the decoder's pool-growth arm),
the findings A and B narrowed for it, and the phase close.

## Hard rules

1. **D-gate-3.** Per commit `gates.sh commit` (+ the commit's in-tree test). Session
   close = the fast set (step 7). No sweeps, no Miri, no benches, no span mid-session.
   **The phase close (step 8) runs `gates.sh exit` unscoped once** — the phase's only
   sweep + Miri + bench + the 7-pair span.
2. **The reference's answer is the test**, and **which reference** is this session's
   first question (step 0). Every feature lands with a referee measured red first.
3. **S48.** Anything not finished stays a loud refusal with a test pinning the code and
   an allowlist row naming the owner.
4. **D-exit-1.** New code is safe Rust where its neighbours allow; a raw signature forced
   by a raw caller carries `// unsafe-cat: port-raw(Phase 9)` + `#[allow(unsafe_code)]`;
   `unsafe_ratchet.sh check` green per commit, deliberate rebaselines in the same commit
   with the reason. **F80's pool growth is different** — it changes a `src/safe/`
   invariant (see step 3); that is safe code and must stay safe.
5. **Do not touch** D-poc-1 (`CompareOutput/39`), screen content (Phase 10's 7 rows),
   any `port-raw(Phase 9)` conversion, F91/F92/F84/F85/F86-open (Phase 9).
6. Commit rhythm `T8b.C<n>`; breadcrumbs to `safety_refactor_log.md` (S31); findings to
   `phase8b_findings.md` from **F97**.

## Current state — verified facts at `65042ac9`

### The downsampler-parity trap — READ FIRST

`libopenh264.a` (what `cxx_enc`, the gtest stretch, and `ecref` all link) is built
`USE_ASM=Yes` on arm64: `nm -gU libopenh264.a | grep -c AArch64_neon` → **160**, and among
them **`DyadicBilinearDownsampler_AArch64_neon`, `DyadicBilinearOneThirdDownsampler_AArch64_neon`,
`DyadicBilinearQuarterDownsampler_AArch64_neon`, `GeneralBilinearAccurateDownsampler_AArch64_neon`,
`GeneralBilinearFastDownsamplerWrap_AArch64_neon`**. Runtime dispatch on this host selects
NEON (`InitDownsampleFuncs`, `downsample.cpp:84–140`). The port has always compared
port-`_c` against this library and been byte-identical — **because no path exercised so
far downsamples**: every sweep and bench runs `iSpatialLayerNum == 1`, where source and
target are the same size and `DownsamplePadding` takes its `WelsMoveMemory_c` branch, not
a kernel. Downsample is the **first kernel family where the port and the reference could
run different code**, and upstream flags it itself: `EncoderOutputTest`'s rows 5 and 7
carry **two** golden hashes with the comment *"Allow for different output depending on
whether averaging is done vertically or horizontally first when downsampling"*
(`encoder_test.cpp:163`, `:174`). SAD/DCT NEON are bit-exact with `_c`; the bilinear
downsamplers may not be.

**So a faithful `_c` port is not guaranteed to match `cxx_enc`.** Resolve it in step 0
before writing any kernel.

### The rows this session owns (18 on the allowlist)

- **EncoderOutputTest**, `HashFunctions.h:31`, rows **4/5/7** (`CompareOutput/4,5,7`):
  row 4 = denoise on, 1 layer (`CiscoVT2people_320x192`, `SM_SINGLE_SLICE, true, 1`);
  row 5 = 2 spatial layers (dual hash); row 7 = 4 spatial layers, 1280×720 (dual hash).
  Rows 8–12 are `BaseEncoderTest.cpp:92` init failures under **S48** (screen content and
  the multi-layer ones) — 8–12 are **Phase 10's / return with these**; confirm each by
  reading, do not assume.
- **Simulcast / multi-layer** (`encode_options_test.cpp`, `encode_decode_api_test.cpp:190`):
  `SimulcastAVC`, `SimulcastAVCDiffFps`, `SimulcastSVC`, `SimulcastAVC_SPS_PPS_LISTING`,
  `DiffSlicingInDlayer`, `DiffSlicingInDlayerMixed`, `Engine_SVC_Switch_I/_P`,
  `AVCSVCExtensionCheck`, `SetOptionEncParamExt/Base`, `GetOptionTid_SVC_L1_NOLOSS/0..2`.
  These need more than one spatial layer to encode, which needs downsample; they pass
  once downsample is byte-correct (or become the honest residue if a specific one needs
  more — say which and why).
- **ParseOnly_General** (`decode_api_test.cpp:873`) — 3 spatial layers (`kiTotalLayer`);
  a pure downsample row now (B did the parse-only half). It returns with downsample.

### Downsample — what to port

- Upstream: `codec/processing/src/downsample/downsample.cpp` (`CDownsampling::Process`,
  `downsample.cpp:144`, dispatches on the ratio to the six function-pointer slots) and
  `downsamplefuncs.cpp` (the `_c` kernels: `DyadicBilinearDownsampler_c` :44,
  `DyadicBilinearQuarterDownsampler_c` :71, `DyadicBilinearOneThirdDownsampler_c` :95,
  `GeneralBilinearFastDownsampler_c` :118, `GeneralBilinearAccurateDownsampler_c` :187,
  `GeneralBilinearDownsamplerWrap` :251). The census lists all of these under 8b.C.
- Port site: `processing/mod.rs`'s `SWelsVpContext` (one field per implemented method;
  `METHOD_DOWNSAMPLE` is in the *"Not translated"* table at `:29–39`) and the caller
  `wels_preprocess.rs`'s `DownsamplePadding` (`:1596`), whose `iSrcWidth != iShrinkWidth`
  branch currently sets `iRet = RET_NOTSUPPORTED` (`:1642`) instead of running a kernel.
  `SPixMap` (`:265`), `Scaled_Picture`/`m_sScaledPicture` (`:198`, `:872`) and the
  `iScaledWidth[]` geometry (`WelsUpdateScaledPicOnDependency`-equivalent, `:726–757`)
  are already in place — this is one method into an existing dispatch, the shape
  `METHOD_VAA_STATISTICS` etc. already have.
- Match the reference's dispatch exactly: which of the six kernels runs is a function of
  the width/height ratio (`Process`'s `if` ladder). Read it; do not guess the boundaries.

### Denoise — what to port

- Upstream: `denoise/denoise.cpp` (`CDenoiser::Process` :66 — three component guards),
  `denoise_filter.cpp` (`BilateralLumaFilter8_c` :41, `WaverageChromaFilter8_c` :89,
  `Gauss3x3Filter` :114). Port site: `wels_preprocess.rs`'s `BilateralDenoising`
  (`:1590`, an empty body behind `bEnableDenoise`) and the `METHOD_DENOISE` table row.
- Only row 4 needs it (denoise on, 1 layer — no downsample), so denoise can be verified
  independently of the NEON question: `bEnableDenoise` has its own `_c` kernels and the
  reference's denoise NEON (`BilateralLumaFilter8_sse2`/neon) is the same identity
  question at smaller scale — check it in step 0 alongside downsample.

### F80 / F87 — the decoder's pool-growth arm

`WelsRequestMem` (`decoder.cpp:464–545`) has three arms; the port has two
(`SyncPictureResolutionExt`, `decoder_core.rs:944`). The missing third arm — **same
resolution, changed ref-list size** (`iCapacity != iPicQueueSize`) — calls
`IncreasePicBuff` (`decoder.cpp:107–163`, 36 stmts) or `DecreasePicBuff` (`:170–235`, 50
stmts), which **grow or shrink the pool in place preserving its live pictures**. The
port answers `dsOutOfMemory` here where the reference decodes cleanly (F87), on
`tests/data/f80/num_ref_change_320x192.264` (24 frames at `iNumRefFrame=1` then 24 at 4;
generator `tools/make_numref_asset.cpp`; the reference's own trace and 48-frame decode
are in F87).
- The port's pool is `safe::Pool<PicSlot>` inside `PicPool` (`pic_queue.rs:121`), whose
  documented contract is **"never grows or shrinks"** (`pool.rs:97`) and whose `Id`s carry
  a debug generation (`pool.rs:58`). This is the one `src/safe/` invariant change in the
  phase: add `Pool::grow(extra: Vec<T>)` and `Pool::shrink_to(keep: &[Id]) -> Vec<T>`
  (or the shape the two C++ functions actually need — read them: `Increase` appends new
  slots and `memcpy`s the old handles; `Decrease` keeps `pPreviousDecodedPictureInDpb`
  first, copies `kiNewSize` handles, frees the rest, and **clears every `pRefPic` entry
  across the new pool** — `decoder.cpp:198–205`). Keep the generation counter honest
  across grow/shrink (a grown slot is generation 0; a dropped slot's id must never
  compare equal to a new one — decide and test, S40's kind of test).
- **The asset moves into `res/` in the same commit** as the fix (`decoder_reachability_sweep.rs`
  globs `res/` and would be red until then), and `ecref_rs --ec=0`-style parity plus a
  dedicated in-tree test (frames, dims, per-call codes and `iBufferStatus`, all from the
  C++) is the referee, measured red first.

### F96 / F88 / F93 — the concealment divergences B narrowed

- **F96** (owner: this session): under `ERROR_CON_DISABLE`, `Error_I_P.264` diverges at
  calls 6 and 16 of 17 — port `dsBitstreamError` (`0x4`) where the reference gives
  `dsRefLost` (`0x2`). B narrowed it to `InitRefPicList` (`decoder_core.rs:4220`) or
  `WelsInitRefList`/`WelsReorderRefList`/`WelsReorderRefList2` beneath it: the reference's
  `InitRefPicList` returns `ERR_INFO_REFERENCE_PIC_LOST` and the arm returns early under
  disabled concealment; the port returns `ERR_NONE` and the slice decode fails instead.
  Two eliminations already done (F96): `WelsCheckAndRecoverForFutureDecoding` (body
  skipped under DISABLE) and `WelsReorderRefList` (its failures set `iErrorCode`,
  observed does not). Referee: `ecref_rs --ec=0` over `res/` (built by
  `tools/ecref/build.sh`). Fix if it is one enumerated behaviour change with a covering
  test; the trace line at `decoder_core.cpp:2713` that names the arm is **not** ported
  (the port emits 4 of 24 trace lines on this stream) — adding it belongs with this fix,
  not before.
- **F88** (owner: this session): a *concealment* divergence under `ERROR_CON_SLICE_COPY`,
  `decoder_ec_test.cpp:302` (`iBufferStatus` 0 vs 1), reproducing at
  `gtest_stretch.sh --seeds=5..5`. Needs the encoder in the loop, so the gtest binary,
  not a `res/` asset. Instrument `DecodeFrameConstruction`'s output decision on that AU.
- **F93** (after F96): parse-only on `Error_I_P.264` — re-measure once F96 lands; its
  extra rows are what parse-only adds on top of F96. Owner: this session if time, else
  the residue.

### The `sp` sweep preset

Defined but not run (`diffharness/sweep.sh:279`, `sweep_ps` — five `eSpsPpsIdStrategy`
values × 3 GOP × cabac). Session B's brief named it `sp`; the tree calls the function
`sweep_ps` and registers it as `ps`. This session **adds a downsample/multi-layer sweep
preset** (the one that actually exercises downsample — `iSpatialLayerNum` 2/4 × denoise
on/off) and runs it at the phase close. Reconcile the name (`sp` vs `ps`) in the charter
and the log; do not leave two.

## Steps

### Step 0 — the downsampler question, answered before any kernel
Build a tiny harness that runs the reference's `_c` and NEON downsamplers over the same
input and diffs the bytes (link `libopenh264.a`, call both `DyadicBilinearDownsampler_c`
and `_AArch64_neon` directly; or drive one encode at `iSpatialLayerNum=2` twice, once
with `WELSCPUFLAG` forced to 0). Do the same for denoise.
- **If `_c` == NEON byte-for-byte** on the ratios these tests use: port the `_c` kernels,
  and byte parity holds against `cxx_enc` as written. Record the proof.
- **If they differ**: decision ladder, do not guess —
  1. If the difference is only the documented vertical/horizontal averaging order and the
     gtest goldens already carry both hashes, port the `_c` kernel and confirm the port
     matches **one** of the two goldens; the diffharness reference must then be built the
     matching way — either force the reference `_c` (a `WELSCPUFLAG=0` env the drivers
     already can pass, or a `USE_ASM=No` reference `.a` for the sweep) — and the choice is
     documented at the preset and in `perf_baseline.md`.
  2. If it is a deeper divergence, **stop and put it in the report as a blocking question
     for the steward** — do not port a kernel that cannot match the referee. Everything
     else in this session (denoise row 4 if denoise is identity-clean, F80, F96, F88) is
     independent and proceeds.
**Accept:** a checked-in proof (a test or a logged measurement) of `_c`-vs-NEON identity
for downsample and denoise, and a one-line decision recorded in the brief's terms.

### Step 1 — denoise (row 4)
Port `BilateralDenoiseLuma`/`WaverageDenoiseChroma`/`Gauss3x3Filter` and the two filter
kernels into a `processing` module; wire `BilateralDenoising`. Referee: row 4's gtest
hash, plus a targeted `rust_enc`/`cxx_enc` pair with `bEnableDenoise` on (the drivers
already expose it — `cxx_enc.cpp:123`). Measured red first.
**Accept:** row 4 passes and leaves the allowlist; the denoise pair byte-identical.

### Step 2 — downsample
Per step 0's decision. Port the six `_c` kernels and `Process`'s dispatch; wire the
`DownsamplePadding` branch. Referee: rows 5 and 7, the simulcast/multi-layer cluster,
`ParseOnly_General`, and a downsample sweep preset (one targeted pair per layer count in
the session; the full preset at the close).
**Accept:** rows 5, 7 and the multi-layer rows that only needed downsample leave the
allowlist; each targeted pair byte-identical against the resolved reference; the residue
(if any) named with why.

### Step 3 — F80 / F87 (the pool grows)
`Pool::grow`/`shrink_to` in `src/safe/pool.rs` with the generation contract decided and
tested; `IncreasePicBuff`/`DecreasePicBuff` ported into `pic_queue.rs`; the third arm
wired into `SyncPictureResolutionExt`. Move the asset into `res/` in the same commit.
Referee: an in-tree parity test on the asset (frames, dims, per-call codes,
`iBufferStatus`, from the C++) + `ecref_rs`, measured red first.
**Accept:** the asset decodes 48 frames matching the reference; `decoder_reachability_sweep.rs`
accepts it; F80/F87 closed.

### Step 4 — F96, then F88, then F93
F96 first (its trace line with it). F88 with the seed-5 repro. F93 re-measured after F96.
Each: fix if one enumerated change with a covering test, else file with the repro and
own it or hand it on. Time-box F88 to 2 h.
**Accept:** F96 closed or owned with a named cause; F88 and F93 progressed and dispositioned.

### Step 5 — census and the last instruments
`port_census.py`: the 8b.C rows go to `present`; regenerate `phase8b_port_census.md`.
Reconcile the sweep preset name (`sp`/`ps`). If time permits, **F95** (make
`find_stub_bodies.py` count inside a leading `abi_guard!`/`panic_probe!` block — a
measurement change, its own commit) since it is the instrument that will watch the C-ABI
slots in Phase 9.
**Accept:** census 8b.C = 0 open; the preset named once.

### Step 6 — what to drop if short
Drop from the end: F95 (→ phase close, Phase 9), then F93, then F88 (keep the seed), then
F96's trace line (keep the fix). Never drop step 0, the downsample/denoise referees, or
F80's test.

### Step 7 — session-close fast set (D-gate-3)
`gates.sh commit`; `gtest_stretch.sh --check` (pinned seed); `abi_exports.sh release`;
`abi_harness/run.sh`; `ecref/compare_all.sh` and `ecref_rs` parity. Log entry;
`phase8b.md` §5's C row.

### Step 8 — the phase close (the only heavy run)
`gates.sh exit` **unscoped** — sweeps in both profiles **including the downsample preset
and `ps`**, both benches, Miri `--lib` whole library + the differential tests; then the
**one 7-pair span** with its null, stated against D-perf-4's tripwire. Write the Phase 8b
CLOSED reconciliation (§0 row, the tally's arc, the findings ledger, what Phase 9 and
Phase 10 inherit) and the exit-condition check against the charter. Confirm the exit
allowlist is **exactly** the 7 Phase 10 rows + D-poc-1 (S48/S47), and every remaining row
is owned by name.

## Do not touch

| item | owner |
|---|---|
| `CompareOutput/39` (POC tiebreak) | D-poc-1 — permanent |
| screen content (7 rows, census 19) | Phase 10 |
| `port-raw(Phase 9)` conversions, F84, F85, F86-open, F91, F92 | Phase 9 |
| the `send-seam`, the context split, the `Send` verdict | Phase 9 |

## Report back

1. Commits (hashes, one line). 2. **Step 0's answer** — the `_c`-vs-NEON measurement and
the decision taken, in full; this is the session's pivot. 3. Tally before/after by
assertion site; allowlist diff (must end at 8 = 7 Phase 10 + D-poc-1). 4. Per step: what
landed, the measured-red evidence, anchors. 5. F80/F87, F96, F88, F93 status. 6. The
phase-close heavy run: exit verdict, sweep/Miri/bench lines, the span table. 7.
Session-close + phase-close gate lines; ratchet deltas with reasons. 8. Findings F97+.
9. Brief facts that did not survive, quoted. 10. What was dropped; the Phase 8b CLOSED
reconciliation and what 9/10 inherit.
