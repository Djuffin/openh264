# Handoff: finishing the Rust encoder port

You are continuing a multi-session, hand-written Rust port of Cisco's OpenH264
**encoder**, in `rust/crates/openh264-rs/`, on branch `rust3`. The decoder port is
mature and passes conformance; the encoder is the work.

**The encoder is byte-identical with the C++ reference across every configuration
the harness can currently drive.** What is left is configurations the port still
refuses, plus cleanup. Read this file, then
[`encoder_port_status.md`](encoder_port_status.md) — that is the living record with
the per-phase log and every defect found so far. Update it before you finish; it is
the only thing the session after you will read.

---

## 0. Ground rules

1. **The C++ in `codec/` is the specification.** Translate statement by statement
   with the original open beside you. Integer widths, shift directions, truncation
   points, evaluation order and *branch order* are load-bearing. Do not "improve"
   algorithms, sort thresholds, or simplify control flow. Phase 5.1 lost a day to a
   port that sorted two thresholds in `RcCalculateGomQp`; the C++ order makes one arm
   dead, and the deadness is the behaviour. Phase 5.3 lost time to a `do`-`while`
   rendered as a `while`.
2. **`codec/` must stay pristine.** `git diff master..HEAD -- codec/` is empty and
   must remain so. You will patch the C++ temporarily for instrumentation; `git
   checkout codec/` and rebuild when done.
3. **No stubs that return success.** If you cannot finish something, leave an
   explicit `todo!()`/`unimplemented!()` or an error return, and say so in your
   report. The tree currently has **zero** of both.
4. **One definition per name** — types, functions, aliases, tables *and* constants.
5. Keep the tree green and commit at every gate. It is green now.
6. Do not touch the decoder except for genuinely shared types/helpers, and then only
   additively. Encoder and decoder legitimately disagree on some names
   (`MAX_SHORT_REF_COUNT`, `MB_TYPE_SKIP`, `DeblockingInit`, `CHROMA_AC`,
   `MAX_PPS_COUNT`). Do not "unify" them.
7. C++ identifiers verbatim (`StashMBStatusCabac`, `pWelsSvcRc`, `iRemainingBits`),
   `#![allow(non_snake_case, ...)]` at module top.
8. **Verify every claim in this file before acting on it.** Every handoff in this
   series has been wrong somewhere. This one already corrects claims the last one
   made (§6). Assume it is wrong somewhere too, and say so plainly when you find it.

---

## 1. Verified current state

`HEAD` = `441ddf3d`, 103 commits on top of `master`. Working tree clean.

```
cargo build                 clean, 16 warnings, all dead_code/unused
cargo test --no-fail-fast   293 passed, 0 failed, 20 ignored
decoder conformance         53/53
codec_unittest              533/534 (DecoderDeblocking.DeblockingInit, pre-existing)
todo!() in src/             0
unimplemented!() in src/    0
codec/ vs master            no diff
```

**Use `--no-fail-fast`.** Plain `cargo test` stops at the first failing binary and
under-reports.

`test_decode_encode_full_cycle_sha1_parity` passes with upstream's own hash and is
the best single end-to-end check you have.

`test_thread_pool_singleton_lifecycle` (`common/wels_thread_pool.rs`) asserts task
completion after a 50 ms `sleep`; it is load-sensitive and will fail if you run the
suite while a `make -j8` is going. Re-run it alone before believing it.

### How far byte-exactness reaches

All measured with `compare.sh` (§2.1):

| axis | configurations | result |
|---|---|---|
| `iRCMode` × input × GOP × init path, CAVLC | 120 | identical |
| the same, CABAC | 120 | identical |
| QP × `iRCMode` × cabac × size (all 52 QPs) | 1560 | identical |
| `SM_FIXEDSLCNUM_SLICE`/`SM_RASTER_SLICE` × 2/3/4/6 slices × mode × GOP × cabac × input | 256 | identical |
| `SM_SIZELIMITED_SLICE` × 5 constraints × 2 QPs × mode × GOP × cabac × input | 320 | identical |

**The one axis still pinned is `iMultipleThreadIdc == 1`.** After that, one spatial
layer and `CAMERA_VIDEO_REAL_TIME`.

---

## 2. The techniques that work

### 2.1 The differential harness

```bash
make -j8 libraries binaries && ./rust/tools/diffharness/build.sh
```

```bash
./rust/tools/diffharness/compare.sh <yuv> <w> <h> <frames> <qp> <cabac> <gop> \
    [rcmode] [baseinit] [slicemode] [slicenum] [threads]
```

- `rcmode` defaults to `RC_OFF_MODE`.
- `baseinit=1` selects `Initialize(SEncParamBase)`, the path upstream's
  `BaseEncoderTest::InitWithParam` takes; it leaves every `FillDefault` value in
  place, so scene-change detection, background detection, adaptive quantisation and
  frame skip are on, `eSpsPpsIdStrategy` is `INCREASING_ID`, and `iTargetBitrate` is
  5 Mbit/s.
- `slicemode` is 0 `SM_SINGLE_SLICE` (default), 1 `SM_FIXEDSLCNUM_SLICE`,
  2 `SM_RASTER_SLICE`, 3 `SM_SIZELIMITED_SLICE`. `slicenum` is the slice count for
  1 and 2, the rows-per-slice for 2, and the byte constraint for 3.
- `threads` is `iMultipleThreadIdc`.

Inputs live in `res/`: `CiscoVT2people_160x96_6fps.yuv`,
`CiscoVT2people_320x192_12fps.yuv`, `Static_152_100.yuv`,
`Cisco_Absolute_Power_1280x720_30fps.yuv`. **There is no 1280x720 CiscoVT2people
file** — an older handoff named one and every sweep that used it silently failed.

Sweep scripts are not in the repo; they are six-line bash loops over `compare.sh`.
Write them fresh.

#### Four traps, each of which has cost real time

- **The drivers take `out.264` BEFORE `rcmode`.** `cxx_enc … -1 0 out.264` silently
  encodes on `RC_QUALITY_MODE`, because `atoi` of a file path is 0. If a dump looks
  like the C++ is ignoring a parameter, check the argument order first.
- **Quote your parameter expansions.** The shell here is zsh, which does *not*
  word-split unquoted expansions, so `for spec in "a b c"; do set -- $spec` leaves
  `$2` and `$3` empty and every invocation runs on garbage arguments. It reads as a
  uniform failure across the whole sweep.
- **Never rebuild while a sweep is running.** `build.sh` replaces `rust_enc` in
  place; a sweep that spans the rebuild reports a phantom failure. One `FAIL=1` in
  this session was exactly that, and re-running clean gave 120/120.
- **A configuration that "passes on the first run" over a newly-enabled feature is
  evidence the feature is not enabled.** See §3.

A non-zero driver exit is reported with the log path. A debug-build Rust panic (an
arithmetic overflow the C++ wraps through, say) otherwise reads as an ordinary byte
difference, because the aborted process leaves a short file.

### 2.2 Link the C++ function and measure it; never hand-derive an expected value

```bash
c++ -std=c++11 -I codec/api/wels -I codec/common/inc -I codec/encoder/core/inc \
    -I codec/processing/interface probe.cpp libopenh264.a -o probe && ./probe
```

Redeclare the function inside `namespace WelsEnc { ... }` — headers are not always
self-contained and a forward declaration links fine. Some symbols are at global
scope (`WelsCalcPsnr`, `WelsCPUFeatureDetect`); check with
`nm -gU libopenh264.a | grep -i <name>` before guessing the namespace.

This established the SPS/PPS bytes, the `offsetof` tables, the `rc.h` layouts, the
32 `SetOption`/`GetOption` return codes, the six `WelsCalcPsnr` values, that
`GetDefaultParams` reports 60 fps, and that `WelsCPUFeatureDetect` returns
`0x00000000` on this machine — which is what licenses this port to skip the SIMD
fast paths and still be bit-exact.

### 2.3 Bisect the bitstream with matched instrumentation

Found every defect in Phases 4.5, 4.6 and 5.1. Patch **both** encoders to print the
same state, `diff`, narrow. The Rust half is permanent, gated through
`encoder::dump_enabled`:

| variable | printed at |
|---|---|
| `OH264_RCDUMP` | per frame in `WelsRcPictureInitGom`, the whole `SWelsSvcRc` |
| `OH264_RCMBDUMP` | per macroblock in `WelsRcMbInitGom`, the whole `SRCSlicing` |
| `OH264_MBDUMP` | per macroblock in `WelsMdInterMbLoop`, **after** `pfMdBackgroundInfoUpdate` |
| `OH264_MEDUMP` | per motion-search call, inputs and result |
| `OH264_FPDUMP` | per macroblock in `WelsMdInterFinePartitionVaa` |
| `OH264_RECDUMP` | per frame in `WelsUpdateRefList`, a checksum of each reconstructed plane |
| `OH264_VPDUMP` | per frame in `AnalyzeSpatialPic`, checksums of every VAA output |

Only the C++ half has to be re-patched. **`git checkout codec/` when done.**

- Place the C++ dump at exactly the same point in the loop. `OH264_MBDUMP` moved to
  *after* `pfMdBackgroundInfoUpdate` precisely so `uiMbType` is what the syntax
  writer sees. One call earlier and `qp=`/`skiprun=` differ for positional reasons
  and look like real defects.
- **`cc=` (`iCostChroma`) and `skip=` (`iCostSkipMb`) are not comparable.** `SWelsMD`
  is an uninitialised stack local in C++ and a zeroed `Default` in Rust. Neither is
  read afterwards. Ignore both.

Order of attack, coarse to fine: per-frame RC/VP state → per-macroblock mode
decision → `BsGetBitsPos(pBs)` after each syntax stage. A difference of N bits
localises to one syntax element.

**Reason from the failure's shape before instrumenting.** Phase 5.3's `qp=0` defect
was found without any dump: *CAVLC-only + low-QP-only + size-dependent* names exactly
one code path, and there is only one CAVLC-only branch in the I-slice loop
(`UpdateQpForOverflow`). It was a wrong constant.

### 2.4 The audits

```bash
rust/tools/find_stub_bodies.py            # call-set audit against the C++ body
rust/tools/find_stub_bodies.py --dups     # duplicated Rust definitions, worst first
rust/tools/find_dup_types.sh              # types, aliases, tables, constants-by-value
```

Both are reading aids, not gates. False positives are cheap (macros, calls through a
local binding, methods on distinct types). False negatives are what matter.

---

## 3. The defect classes — read this before writing any code

**A function that exists is not a function that works, and a function that works is
not a function that runs.** Eight phases running, this is the dominant defect class.
Five shapes, all of which you should expect again:

| shape | examples | what finds it |
|---|---|---|
| a body that does a fraction of the work under the correct name | `WelsEncoderApplyFrameRate`, `WelsEncoderParamAdjust` (12 lines against 296), `WelsCabacInit`, `OutputPMbWithoutConstructCsRsNoCopy` | `find_stub_bodies.py`'s call-set audit; reading the C++ beside it |
| a faithful body with **no call site** | `GomRCInitForOneSlice`, `WelsCabacInitContexts`, `WelsCabacContextInitFromContexts` | nothing automated — only diffing the *caller* against the C++. Narrowing the `dead_code` allows would surface these |
| a faithful body **shadowed** by a same-named one in the module that uses it | `WelsMdUpdateBGDInfo`, `BsAlign` (missing its `BsFlush`), `WelsGetNextMbOfSlice`, `GetCurrentSliceNum`, `WelsInterMbEncode` | `find_stub_bodies.py --dups` |
| a correct function reached through a **wrong constant** | `GOM_SAD`/`GOM_VAR`, `DELTA_QP` (1 where `rc.h` says 2), `MAX_FRAME_RATE` (30 where the header says 60), `g_kuiGolombUELength` | `find_dup_types.sh`'s value comparison |
| a correct function under a **configuration that never runs** | the whole CABAC path, silently downgraded to CAVLC by `PRO_BASELINE`; both dynamic-slicing MB loops | checking that the knob changes the **C++** output |

Two corollaries earned the hard way:

> **A fix applied to one of a pair of near-identical functions does not reach the
> other, and nothing notices while the other is unreachable.** `sMd.bMdUsingSad` was
> fixed in `WelsPSliceMdEnc` phases ago; its twin `WelsPSliceMdEncDynamic` still had
> the defect when Phase 5.3 finally ran it. When you fix anything, grep for the
> `…Dynamic`, `…Ext`, `…_c` and enhancement-layer variants **before** moving on.

> **Before trusting a sweep over a newly-enabled feature, check that turning the knob
> changes the C++ output.** Both drivers pinned `uiProfileIdc = PRO_BASELINE`, and
> `ParamValidationExt` (`encoder_ext.cpp:655`) resets `iEntropyCodingModeFlag` to 0
> for a baseline layer. A 120-configuration "CABAC sweep" passed with `8034 == 8034`
> and not one CABAC symbol was ever written. Four real defects were hiding behind it.

A third, about counting:

> **A switch-arm count is not a coverage measure.** Six of the twelve `SetOption`
> options the port already "handled" called helpers that were stubs, so three options
> were listed as done and were wrong.

---

## 4. The work, in order

### 4.1 `iMultipleThreadIdc > 1` — the last refused configuration, and the big one

This is the only axis left that the harness can drive and the port refuses.

**The ground truth is already measured, so you do not have to re-establish it:**

- **The C++ multi-threaded encoder is deterministic.** Three runs each at
  `iMultipleThreadIdc` 2 and 4 produced identical bytes, for both
  `SM_FIXEDSLCNUM_SLICE` and `SM_SIZELIMITED_SLICE`. `compare.sh` is a valid oracle.
- **Multi-threaded output differs from single-threaded** for the same nominal
  configuration — 8324 bytes against 8322 at `SM_FIXEDSLCNUM_SLICE`/4 slices.
  Threading is not a scheduling detail here: it changes the slice partitioning and
  therefore the bitstream. **Compare against the multi-threaded C++, not the
  single-threaded one.**

**Do not trust the inventory.** More of this exists than you would guess:
`slice_multi_threading.rs` (903 lines), `wels_task_management.rs` (865) and
`common/wels_thread_pool.rs` (905) total 2673 lines against 948 lines of C++ in
`slice_multi_threading.cpp` + `wels_task_management.cpp`, and `InitAllSlicesInThread`,
`SliceLayerInfoUpdate`, `AdjustBaseLayer`, `AdjustEnhanceLayer`, `CreateTaskManage`
and `AppendSliceToFrameBs` all have bodies. **Every one of those bodies is
unverified.** Phase 5.3 found that both dynamic-slicing MB loops — ported, long,
plausible, structurally right — were badly wrong because nothing had ever executed
them. The MT bodies are in exactly that state. Diff each against the C++ before
believing it.

What is definitely missing:

- **`RequestMtResource`** (`slice_multi_threading.rs:550`) allocates `SSliceThreading`,
  `pThreadPEncCtx` and `pThreadBsBuffer` and stops. The C++ (111 lines against the
  port's 55) then opens four event sets per thread — `pUpdateMbListEvent`,
  `pFinUpdateMbListEvent`, `pSliceCodedEvent`, `pReadySliceCodingEvent` — and creates
  the threads.
- **`pTaskManage` is never created** in `WelsInitEncoderExt`.
- **Three refusal sites** in `encoder_ext.rs`: `:1151` (`RequestMtResource`), `:3227`
  (the `AdjustEnhanceLayer`/`AdjustBaseLayer` load-balancing arm under
  `SM_FIXEDSLCNUM_SLICE`), `:3422` (the MT encode branch —
  `encoder_ext.cpp:3714`, `pCtx->pTaskManage->ExecuteTasks()` then
  `AppendSliceToFrameBs`).

Suggested order: get `iMultipleThreadIdc = 2` with `SM_FIXEDSLCNUM_SLICE` and 2
slices byte-exact on one frame first, then widen. The single-threaded path must stay
green throughout — re-run the 120-config CAVLC sweep after every commit.

### 4.2 `METHOD_DOWNSAMPLE`, and with it multi-layer SVC

The largest **untested** area of the encoder: more than one spatial layer.
`processing/mod.rs` still returns `RET_NOTSUPPORTED` for `METHOD_DOWNSAMPLE`, which
is what blocks it. `ParamBaseTranscode` was corrected in Phase 5.3 for exactly this
(it wrote `sSpatialLayers[idx]` where the C++ writes `[0]`), so that pre-requisite is
out of the way.

Expect the harness to need a `layers` argument, and expect `SPS_LISTING` (§4.4) to
become relevant at the same time.

### 4.3 `SCREEN_CONTENT_REAL_TIME`

Interlocking, so treat it as one unit:

- `METHOD_SCENE_CHANGE_DETECTION_SCREEN`, `METHOD_COMPLEXITY_ANALYSIS_SCREEN`,
  `METHOD_SCROLL_DETECTION` — the screen scene-change detector reads the scroll
  detector's result.
- `PreprocessSliceCoding` does not translate `encoder_ext.cpp:2708-2771`.
- `AllocPicture` refuses `iNeedFeatureStorage != 0` (`encoder_ext.rs:837`) rather
  than calling the unported `RequestScreenBlockFeatureStorage`.
- `RequestMemorySvc` refuses the `SVAAFrameInfoExt`/`RequestMemoryVaaScreen`
  allocation (`encoder_ext.rs:1239`).
- `GOM_H_SCC` is corrected (8, not 2) and waiting.

### 4.4 The three `SPS_LISTING` parameter-set strategies

`CreateParametersetStrategy` returns null for `SPS_LISTING`,
`SPS_LISTING_AND_PPS_INCREASING` and `SPS_PPS_LISTING`, and
`WriteSavcParaset_Listing` refuses (`encoder_ext.rs:2747`).
`paraset_strategy.rs`'s C-style vtable is the established pattern.
`WelsEncoderParamAdjust`'s `OutputCurrentStructure`/`LoadPreviousStructure` calls are
live for the non-`CONSTANT_ID` strategies, so the `SPS_LISTING` overrides of those two
are what the new strategies need first.

### 4.5 `METHOD_DENOISE`

Self-contained; the smallest remaining VP method.

### 4.6 `bEnableAdaptiveQuant` allocation

`encoder_ext.rs:1251` refuses the `sAdaptiveQuantParam` buffers. Note
`ParamValidation` (`encoder_ext.cpp:301`) sets `bEnableAdaptiveQuant = false`
unconditionally — "turn off adaptive quant now, algorithms needs to be refactored" —
so this is unreachable in upstream too. `processing/adaptive_quantization.rs` is
correct-but-unreachable for the same reason. Low priority; do not "fix" the flag.

---

## 5. Cleanup (Phase 6) — about a third done

Do these alongside the above, not after.

1. **`find_stub_bodies.py --dups`: 90 names, most unread.** Four of the seven
   inspected across Phases 5.2 and 5.3 were real defects. **This is the
   highest-yield unfinished audit in the tree.** Work down the list worst-disparity
   first. For each: is one body a truncation of the other, and which one do the call
   sites actually resolve to? Delete the loser, or add a one-line comment saying why
   two definitions are correct. Known-benign: `Process`/`Init`/`Set`/`Get`/`Execute`
   and the thread-pool container helpers (methods on distinct types), the `Bs*`
   writers (compared line by line in Phase 3), and the
   `WelsI16x16LumaPred*_sse2`/`_neon` pairs (all unassigned on this target).
2. **`find_dup_types.sh` reports 130 duplicated names** (23 types, 40 aliases,
   29 tables, 38 constants) across 269 lines of output. Most are one encoder
   declaration beside one decoder declaration of a name the codecs genuinely keep
   separate; those want a one-line comment, not a merge. Encoder-vs-encoder pairs can
   be re-exported the way `MAX_PPS_COUNT`, `MAX_FRAME_RATE`, `DELTA_QP`,
   `MAX_SLICES_NUM`, `GOM_H_SCC`, `RECIEVE_FAILED` and the `WELS_CPU_*` set now are.
   **The gate "`find_dup_types.sh` silent" is not reachable** without violating rule
   6; aim for "every entry either collapsed or annotated" instead.
3. **Two constants are genuinely wrong and live, both decoder-side**, both surviving
   conformance:

   | constant | wrong copy | header | effect |
   |---|---|---|---|
   | `MAX_MB_SIZE` | `decoder/parameter_sets.rs` = 1024 | `dec_golomb.h:338` = **36864** | `nalu.rs:1481` rejects an SPS with `iMbWidth > 1024`. Stricter than the reference, so it rejects nothing real — but a divergence, not hardening. |
   | `MAX_NAL_UNIT_NUM_IN_AU` | `decoder/decoder_core.rs` = 1024 | `wels_const.h:59` = **32** | sizes the access-unit NAL list (`MemInitNalList`) and seeds `uiCountUnitsNum`; the port allocates 32× the reference and grows on a different schedule. |

   Also 19 decoder `ERR_INFO_*`/`ERR_LEVEL_*` codes genuinely disagree across decoder
   modules. All of this is decoder territory — rule 6 — so fix additively and re-run
   conformance.
4. **67 modules carry a `#![allow(...)]` blanket; 63 silence `dead_code`.** Fold them
   back to the narrowest scope that still compiles. These hide exactly the warning —
   `dead_code` on a function that should have a caller — that would have flagged
   `GomRCInitForOneSlice`, `WelsCabacInitContexts` and
   `WelsCabacContextInitFromContexts`, all faithful bodies nothing called. **This is
   the only automated thing in the tree that can find the no-call-site defect class,
   which has now bitten five times.** Doing it early pays for itself.

---

## 6. Claims this handoff corrects

So you know the failure mode of these documents:

- The Phase-5.2 handoff said CABAC needed five unported functions in
  `set_mb_syn_cabac.cpp`. It needed **three**, in `set_mb_syn_cavlc.cpp` — and four
  more defects that no reading would have found, because the harness was not
  exercising CABAC at all.
- It listed `res/CiscoVT2people_1280x720_30fps.yuv` as an input. **That file does not
  exist**; the 720p input is `res/Cisco_Absolute_Power_1280x720_30fps.yuv`.
- It described `iMultipleThreadIdc > 1` and `SM_SIZELIMITED_SLICE` as needing named
  functions ported. True, but in both cases the already-ported, never-executed
  supporting code was the larger problem.
- `encoder_port_status.md` claimed Phase 5.2 left "every duplicated `pub const` in
  `src/` holding the same value in every module". That is true for `src/encoder/`,
  not for `src/`; 38 still differ. Corrected in the doc, and see §5.3 above.
- The Phase-5.1 session reported the `SetOption` gap as 14 options, then 20. It was
  **20 switch arms plus six stubbed helpers**.

---

## 7. Definition of done, and reporting

**Full parity** means all of:

1. `compare.sh` exits 0 across the cross-product of: 5 `iRCMode` × both init paths ×
   cabac 0/1 × the four inputs × GOP −1/2/8 × all four slice modes ×
   `iMultipleThreadIdc` 1/2/4, plus all 52 QPs at one size.
2. No configuration the C++ accepts is rejected by the port, and none is accepted and
   silently mishandled. Every remaining refusal is a documented C++ refusal too.
3. `todo!()` and `unimplemented!()` both zero in `src/`.
4. `find_stub_bodies.py --dups` reviewed to zero unexplained entries;
   `find_dup_types.sh` entries all collapsed or annotated.
5. `cargo test --no-fail-fast` green with no `#[ignore]` that hides a port defect —
   each remaining one carries a measurement explaining why, not a guess.
6. `codec_unittest` still 533/534 and decoder conformance still 53/53.
7. `git diff master..HEAD -- codec/` empty.

At each gate report: which criteria you met **with actual command output**; what you
could not do and why; and every place you deviated from the C++, with justification.
If a claim in this file or in the status doc is wrong, say so plainly and proceed from
what the code actually does — then correct the document.

> One deliberate deviation exists today and should stay: `rc_mode_from_raw`
> (`wels_encoder_ext.rs`). C++ casts a raw `int32_t` into `RC_MODES`, so an
> out-of-range value is stored verbatim. A Rust enum cannot hold that; an
> unrecognised mode becomes `RC_QUALITY_MODE` (C++'s 0). Every value the reference
> accepts round-trips exactly.
