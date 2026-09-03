# Phase 10 findings

*Numbering continues Phase 9's (which closed at F314). Each entry is a fact with the
grep or the run that produced it. Session P10.1 opens the file.*

## F315 — `VaaBlock` (B1) does not build without the raw-pointer deletion (B2): the fork's `thread::scope` is the `Sync` referee (P10.1)

The brief sequenced B1 (the enum in the context) and B2 (delete
`SComplexityAnalysisScreenParam::pGomComplexity: *mut i32`) as two checkpoints, with
B1 "changing no behaviour" and a `vaa_block_is_sync` test that "will not compile
until B2". The test was the least of it. With `pVaa: Option<Box<VaaBlock>>` the
context's `Sync` depends on `SVAAFrameInfoExt`, the raw pointer makes it `!Sync`, and
`cargo build` failed at every `thread::scope` that shares the context
(`slice_multi_threading.rs:1902`, `:2433` — E0277 "`*mut i32` cannot be shared
between threads safely", with the chain of `-->` lines running through
`wels_preprocess.rs:499` (the field), `:709` (`SVAAFrameInfoExt`), `:754`
(`VaaBlock::Screen`) and `encoder_context.rs:1850` (the field)). So B1 and B2 landed
as one commit (`00bbe73d`), and D-scc-5's claim — the workers can share the
extension because it is `Sync` by construction — was enforced by the compiler
before any test asserted it.

## F316 — two live size pins the brief said did not exist, both failing the build at const-eval (P10.1)

* "There is no live `assert_size!(SDqLayer, ..)`" — there is
  `assert_size_by_profile!(SDqLayer, debug 800, release 728)` (`abi_guard.rs`), which
  the brief's own grep pattern `assert_size!(SDqLayer` cannot match. Adding
  `pFeatureSearchPreparation: Option<Box<..>>` (B4) failed the build with
  `evaluation panicked: SDqLayer (debug)`; moved to 808/736, both measured.
* `assert_size!(SScreenBlockFeatureStorage, 160)` (`abi_guard.rs:152`), not mentioned;
  deleting `pFeatureOfBlockPointer: Vec<u16>` (B5, D-scc-3) failed with
  `SScreenBlockFeatureStorage must match the C++ struct size`; moved to 136, measured.

Neither is a defect — both pins did their job — but a brief that says "verify with
`grep -rn "assert_size!(SDqLayer"`" and then "before relying on it" was relying on
the wrong grep.

## F317 — `InitDqLayers` is two loops, and the brief's `kiWidth`/`kiHeight` are the first loop's (P10.1)

B4's allocation goes in the DQ-layer loop (`encoder_ext.rs:788`), which the brief
placed "before the layer is stored at `:866`" and sized with "`kiWidth`/`kiHeight`,
already bound at `:730-731`". Those bindings belong to the reference-list loop
(`:726`); the DQ-layer loop binds `kiMbW`/`kiMbH` from the parameter block and
nothing in pixels. The C++ has the same two loops and passes
`pDlayer->iVideoWidth`; the port copies the two scalars out of the parameter block at
the site (§4.6). E0425 at first build, then correct.

## F318 — the init-failure referees cannot tell the three screen refusals apart; a probe can (P10.1)

`InitializeExt` maps every `WelsInitEncoderExt` failure to `cmInitParaError` (1), and
none of the three port-added refusals logged. So the A5 hand row's Rust trace log
before B3 and after B3 are byte-identical (6 lines each, the last the driver's
`InitializeExt returned 1` assertion), and "prove the failure moved" — the brief's
B3 check — needed an instrument the tree does not carry. A temporary `eprintln!`
at the three sites (reverted before the commit) read, on the A5 row after B3:

    PROBE: RequestMemorySvc built VaaBlock::Screen rows=1 stride=960
    PROBE: AllocPicture refused iNeedFeatureStorage=0x307
    PROBE: InitDqLayers: AllocPicture answered None for layer 0

`rows=1` is `iMaxNumRefFrame` for a single-temporal-layer screen encode
(`WelsCheckNumRefSetting`: `WELS_MAX(1, uiGopSize >> 1)`), `stride=960` is
`iCountMaxMbNum << 2` at 320x192. After B4 the site is unchanged, because the
reference-list loop that calls `AllocPicture` precedes the DQ-layer loop that builds
the preparation — in the C++ and the port alike — so the brief's "prove it moved
again" after B4 has nothing to move; B4's referee is its unit test.

## F319 — the SCD P-skip judging arm is entered on every P macroblock of a screen encode; the skips it decides stay dark (P10.1, S55)

The brief says to keep the `SCREEN_CONTENT(dormant)` tags on "the SCD family" as
still unreachable, and the seven F125 tag blocks in `svc_mode_decision.rs` said the
axis is one "neither diffharness driver expresses". After A1 both drivers express it
and after B5 `WelsInitSCDPskipFunc` installs the judging arm for every `scc` row
(`bScreenContent && bEnableSceneChangeDetect && iComplexityMode < HIGH` is true:
screen usage forces scene-change detection on). Measured with a temporary
`eprintln!` at the head of `WelsMdInterJudgeSCDPskip` and at the `SvcMdSCDMbEnc`
call, positive and control, both reverted:

| row | `WelsMdInterJudgeSCDPskip` | `SvcMdSCDMbEnc` |
|---|---|---|
| A5 hand row (scc_text_320x192_k3, 59 P frames x 240 MBs) | **14160 / 14160** | 0 |
| camera control (CiscoVT2people_320x192, 8 P frames x 240 MBs) | 0 / 1920 | 0 |

So the arm is live and the decisions are not: `SetBlockStaticIdcToMd` reads a
selector (`pVaaBestBlockStaticIdc`) that only P10.2's screen scene-change plugin
stamps, and `JudgeScrollSkip` a scroll flag only P10.2's scroll detector sets, so
both `Judge*Skip` answer false and `SvcMdSCDMbEnc` — the body that would change
bytes — is entered zero times. The seven tag blocks are reworded to say exactly
this; the tags stay, on the brief's ruling and on this measurement.

## F320 — upstream's `ScreenContentScrollMotionVectorBounds` passes on the C++; the port's copy asserted the opposite on a false premise (P10.1)

`tests/api_param_bounds_test.rs`'s copy asserted `CM_INIT_PARA_ERROR` with a comment
that 1800 is not a multiple of 16 and `ParamValidationExt` (`encoder_ext.cpp:521`)
rejects it, "verified against the C++ reference encoder". Re-measured this session:

    ./codec_unittest --gtest_filter=EncoderInitTest.ScreenContentScrollMotionVectorBounds
    [       OK ] EncoderInitTest.ScreenContentScrollMotionVectorBounds (28 ms)

`ParamTranscode` aligns the layer size to 16 and crops (`param_svc.h:486-489`), so
the `& 0x0F` check sees 1808. The port's copy had also left `iRCMode` at its default
where upstream sets `RC_OFF_MODE`, which is the rejection it could actually have
seen. Rewritten to mirror `encoder_test.cpp:303-360` exactly (init 0, two
`EncodeFrame`s returning 0); it passes since B5, and its allowlist row left in the
same commit.

## F321 — `CompareLine` answers "different" on equal rows when the width is at or below 12 (P10.2, upstream)

`ScrollDetectionFuncs.cpp:99-108` seeds `int32_t iCmp = 1` and clears it only in the
`kiWidth > 12` branch:

```c
int32_t iCmp = 1;
if (LD32 (pYSrc) != LD32 (pYRef)) return 1;      // three of these: 12 bytes
...
if (kiWidth > 12)
  iCmp = WelsMemcmp (pYSrc + 12, pYRef + 12, kiWidth - 12);
return iCmp;
```

So a caller passing `kiWidth <= 12` gets 1 — "the rows differ" — even when all twelve
bytes it compared are equal, and `kiWidth == 12` is on the wrong side of the test. A
second behaviour rides with it: the three `LD32`s run *before* `kiWidth` is consulted,
so twelve bytes are read from each row however narrow the nominal comparison is.

Both are ported as written (`processing/scroll_detection.rs`) and pinned by
`compare_line_keeps_its_twelve_byte_floor`. **The brief was wrong here**: it said to
"compare `kiWidth.max(12)` bytes", which answers 0 in exactly the case upstream
answers 1.

Unreachable from the encoder. `ScrollDetectionWithoutMask` passes `iWidth =
kiRegionWidth / 2 = (w - 2 * (h >> 4)) / 6`, which on the smallest `scc` geometry
(160x96) is `(160 - 12) / 6 = 24`; the mask path refuses anything at or below
`MINIMUM_DETECT_WIDTH = 50` before halving.

## F322 — the scroll detector reports a **zero** vector on an unchanged frame, not "no scroll" (P10.2)

`ScrollDetectionWithoutMask`'s loop breaks on `bScrollDetectFlag && iScrollMvY`, so a
region that matches its own row at `iOffsetAbs == 0` does *not* stop the walk: all
nine regions run and each reports `bScrollDetectFlag = 1, iScrollMvY = 0`. The two
states are different inputs to the screen scene-change detector's
`(!iScrollMvX || !iScrollMvY)` guard — which both satisfy, so a still frame takes the
scrolled-SAD branch with a zero vector and measures the collocated block twice.

Pinned by `an_identical_pair_detects_a_zero_vector`. Not a defect; recorded because
"detected" reads as "scrolled" and here it does not mean that.

## F323 — the two screen plugins disagree about the sign of the same scroll vector, and one of them does not bound the read it guards (P10.2, upstream)

For one `SScrollDetectionParam` produced by one detector:

| plugin | the read | the bounds test |
|---|---|---|
| `CSceneChangeDetectorScreen` (`SceneChangeDetection.h:170`) | `pRefTmp + iScrollMvY * iRefStride + iScrollMvX` | `iBlockPointY + iScrollMvY` in `[0, iHeight - 8]`, block is 8x8 — **agrees** |
| `CComplexityAnalysisScreen::GomComplexityAnalysisInter` (`ComplexityAnalysis.cpp:451`) | `pTmpRef - iScrollMvY * iStrideX + iScrollMvX` | `iBlockPointY + iScrollMvY` in `[0, iHeight - 8]`, block is **16x16** — **does not** |

The complexity kernel subtracts on Y where the scene-change kernel adds, and its guard
tests the *plus* offset against an *8-sample* margin for a *16-sample* read. A
macroblock near the top of the picture with a positive vector passes the test and reads
above the picture; one near the right edge can read eight columns past it.

**Unreachable, and the reason is the caller rather than the plugin.**
`AnalyzePictureComplexity` zeroes `sScrollResult` before every `Set`
(`wels_preprocess.cpp:863-865`, and the port's screen arm), so `bScrollFlag` is false on
every call the encoder makes and the entire branch is dark. Ported literally; if
anything ever reaches it, `PlaneCursor`'s bounds check is the referee and a panic there
is this finding rather than a new mystery.

## F324 — a block-static row the port cannot resolve is a refusal where the C++ writes through `NULL` (P10.2, D-scc-8)

`CSceneChangeDetectorScreen::operator()` post-increments through
`m_sSceneChangeParam.pStaticBlockIdc` once per 8x8 block with no null test. Two callers
can hand it a null: `DetectSceneChangeScreen` copies
`pVaaExt->pVaaBlockStaticIdc[iScdIdx]`, and `UpdateBlockStatic` copies
`pVaaExt->pVaaBestBlockStaticIdc` — both null when the store is unallocated.

The port carries the row *selector* in `SSceneChangeResult::pStaticBlockIdc` (S12.3) and
resolves the row with `SBlockStaticIdcStore::row_mut`, which answers `None` in exactly
those states. `CSceneChangeDetectionScreen::Process` additionally returns
`RET_INVALIDPARAM`, **without writing anything**, when the row is shorter than the block
grid — where the C++ walks off the end of the allocation.

Three defined refusals replacing undefined behaviour. At the first call site the
refusal is spelled as a non-zero `ret`, taking the branch a failing `Process` would; at
the second it is silent, because the C++ discards that call's return value and there is
nothing to report it to. Pinned by
`a_short_block_static_row_is_refused_without_a_write`.

## F325 — `CComplexityAnalysisScreen`'s two intra SADs are named backwards upstream (P10.2)

`m_pIntraFunc[0]` is `WelsI16x16LumaPredV_c` — the *vertical* prediction — and
`ComplexityAnalysis.cpp:373` stores its cost in the variable called `iBlockSadH`;
`m_pIntraFunc[1]` is the horizontal prediction and its cost goes in `iBlockSadV`. Only
`WELS_MIN` of the pair is ever read, so the swap cannot change a value and is invisible
in the C++. The port names its locals for what they hold (`iSadPredV`, `iSadPredH`) and
carries the crossreference in a doc comment, because a reader checking the port against
the C++ line by line will otherwise think one of them is a transcription error.

## F326 — the screen complexity plugin cannot overrun the rate controller's GOM array, on either side (P10.2)

The brief asked whether the C++ can write past `pWelsSvcRc->pCurrentFrameGomSad` and
said the Rust must refuse if so. It cannot. The plugin writes
`ceil(iMbHeight / iMbRowInGom)` buckets with `iMbRowInGom = GOM_H_SCC = 8`;
`RcInitSequenceParameter` sets

    iGomSize = ceil(iNumberMbFrame / iNumberMbGom) = ceil(iMbHeight / iGomRowMode0)

(`ratectl.cpp:153-165`) where `iGomRowMode0` is interpolated between `GOM_ROW_MODE1_*`
(1 or 2) and `GOM_ROW_MODE0_*` (2 or 4) and so never exceeds **4** (`rc.h:98-107`).
`4 < 8`, so `iGomSize` always covers the plugin's bucket count, for every geometry and
every `iRcVaryRatio`. (`bMultiSliceMode` reassigns `iNumberMbGom` *after* `iGomSize` is
computed, so it does not enter.)

The port keeps a guard anyway — one comparison per frame, and a `WELS_LOG_ERROR` plus an
early return rather than a mid-frame panic if the analysis above is ever falsified by a
change to either constant.

## F327 — the verdict referee: what is provable before the bytes are (P10.2, S55)

Under `SCC_TIER=min` rate control is off, so every macroblock on both sides is coded at
the driver's QP 26 and `iFrameAverageQp` is 26 everywhere. Nothing the screen
preprocessor reads depends on the coded bytes, so its inputs are identical frame by
frame *while the bitstreams differ by design*. That makes the sequence of
`iVaaFrameSceneChangeIdc` verdicts a referee this session could turn green, three
checkpoints before P10.3's byte gate. `rust/tools/diffharness/scc_verdicts.sh`.

Its calibration, all three points measured:

| point | tally | reason |
|---|---|---|
| C1, before the port printed anything | 0/28 | every Rust extract **empty**; C++ 30-62 lines per row |
| C2, verdict line ported, plugins not | **18/28** | content |
| C6, plugins wired | **28/28** | — |

**The brief predicted 0/28 at C2 and the real number is 18/28.** Twelve rows agree by
accident and six by configuration: with no plugins the port's judgement loop never runs,
so both counters stay 0 and every verdict is `SIMILAR_SCENE` — which is also the C++'s
answer on the three camera-derived clips, on `scc_text_160x96_k1` (generated with
`--cut-every 0`, so it has no cuts), and on `scc_text_320x192_k17 gop=4`, whose 4-frame
intra period turns the cut frames into IDRs before the detector sees them. Only ten rows
discriminate, all of them synthetic clips that cut. A referee's denominator is not its
strength: quote the rows that can fail.

## F328 — the F125 tags come off, and the measurement that took them off (P10.2, C8)

F319 measured the SCD P-skip judging arm *entered* on every P macroblock of a screen
encode with every decision it makes answering false, because `SetBlockStaticIdcToMd`
read a selector nothing stamped and `JudgeScrollSkip` a flag nothing set. P10.2's three
plugins stamp both. A temporary entry counter (F126's pattern) on one `scc` row —
`scc_text_320x192_k3`, 60 frames, gop -1, cabac 0, RC off — measured, and was reverted
in the same commit:

| site | entries | was (F319) |
|---|---|---|
| `WelsMdInterJudgeSCDPskip` | >= 13000 | 14160 (already live) |
| `SetBlockStaticIdcToMd` | >= 13000 | live, `row` = `None` |
| `MdInterSCDPskipProcess` | >= 25000 | — |
| `CalUVSadCost` | >= 22000 | 0 |
| `JudgeStaticSkip` | >= 13000 | live, always false |
| `JudgeStaticSkip` -> **true** | >= 1000 | **0** |
| `JudgeScrollSkip` | >= 11000 | live, always false |
| `JudgeScrollSkip` -> **true** | >= 9000 | **0** |
| `SvcMdSCDMbEnc` | >= 11000 | **0** |

(Lower bounds: the counter reported every thousandth entry.) `SvcMdSCDMbEnc` — the body
that changes bytes, and the one that read 0 in all 48 `bg` rows — encodes thousands of
macroblocks now. Ten of the thirty `SCREEN_CONTENT(dormant: Phase 10)` tags are retired
on this: the seven F125 blocks, the `SVAAFrameInfoExt` module tag, the `pfMotionSearch`
dispatch tag (whose *index* is live now, though its four entries are still identical
until P10.3 installs the variants), and `rc.rs`'s raw-pointer table row. Twenty stay,
sixteen of them the FME and feature-search family in `svc_motion_estimate.rs`, which is
P10.3's whole subject.

## F329 — the screen dispatch's two unconditional dereferences, and the parameter checks that keep the port's guards dark (P10.3, D4)

`PreprocessSliceCoding`'s screen block dereferences two pointers without asking,
and the port asks at both. Neither guard can fire, and the reason in each case is
a check in a different file:

| `encoder_ext.cpp` | the C++ | the port | what makes it unreachable |
|---|---|---|---|
| `:2742` | `pCurLayer->pRefPic->pScreenBlockFeatureStorage` | `kbFmeInstalled = false` | `AllocPicture`'s screen arm attaches a storage to every reconstruction picture under `SCREEN_CONTENT_REAL_TIME` (P10.1.B5), and the block runs only on a P slice, where a reference is bound |
| `:2767-2768` | `pCurLayer->pFeatureSearchPreparation->..` on the I-slice arm | the reset is skipped | `ParamValidation` refuses screen content above one spatial layer (`:274-279`), so the only DQ layer *is* the last one — which is the layer `InitDqLayers` gives a preparation (`:1125-1135`, `iDlayerIndex == iDlayerCount - 1`) |

Recorded because the guarantee is three files away in each case, and it is the
kind that erodes: the day upstream allows a second spatial layer for screen
content, the second row becomes a null dereference there and a **silently skipped
reset** here — the port would carry `bFMESwitchFlag` and `uiFMEGoodFrameCount`
across an IDR instead of restoring them. That is a behaviour difference, not a
crash, which is the harder kind to notice. The evidence that neither fires today
is the byte gate: `sweep.sh scc` reads 148/148 in both profiles.

## F330 — the FME preprocess moved above the function-pointer table's `&mut`, and the one-writer proof that says it may (P10.3, D-scc-15)

The C++ writes four table slots, runs `PerformFMEPreprocess`, then writes two
more. The port cannot: `fl` is `&mut` derived from `pCtx`, and the preprocess
reaches the layer, the reference list, the VAA block and the picture pools —
every one of them through `pCtx` (§4.6). So the step runs **before** the borrow
is taken.

The reorder is behaviour-preserving, and the argument is a grep rather than an
inspection. The step reads the table only through `FmeKernels::of`, which is
exactly three slots — `pfCalculateBlockFeatureOfFrame[2]`,
`pfInitializeHashforFeature`, `pfFillQpelLocationByFeatureValue`. Each has
**exactly one writer in either tree**: `WelsInitMeFunc`, at initialisation
(`svc_motion_estimate.rs:589-592`; `SetFastCodingFunc` and `SetNormalCodingFunc`
write four other slots each, and the P-slice block writes none of the three).
Symmetrically, the step writes only the reference's storage and the layer's
preparation, which no table write reads. So the two halves commute, and the
measured 148/148 is the check on the argument rather than its substitute.

The same shape is why `SetMeMethod`'s two `SetMeMethod(...)` calls stay *inside*
the table borrow and the `SFeatureSearchPreparation` half does not: the split is
along "does this touch the context", not along the C++'s statement order.

## F331 — the wide `scc` tier and the five gtest rows closed with the min tier, not after it (P10.3, D6)

P10.3's charter was the 28-row `min` tier — rate control off, one slice, one
thread, no LTR — and P10.4 was to be "widen and close": rate control on
(`MIN/MAX_SCREEN_QP`, screen complexity into `RcCalculatePictureQp`),
`SM_SIZELIMITED_SLICE` with four threads, lossless LTR
(`CWelsReference_LosslessWithLtr`, `GetRefFrameInfo`, `UpdateBlockStatic`, the
source-pool plane source for the FME preprocess).

Measured after D4: **`sweep.sh scc` reads `PASS=148 FAIL=0` in both profiles** —
the whole tier, all four axes, on the first run. The five
`EncodeFile/EncoderOutputTest.CompareOutput/8..12` rows pass too (`--check`
reported them "LISTED BUT PASSING"; 193/199 -> 198/199), and they are the harder
half: `GetDefaultParams` leaves rate control **on** for all five, `/11` and `/12`
run four threads, `/12` adds lossless LTR (`test/api/BaseEncoderTest.cpp:30-66`).

Why the axes needed nothing of their own is worth stating, because "it passed"
is not an explanation. Every one of them was already byte-exact on the *camera*
path — `sweep.sh` has run `rc`, `sl`, `mt` and `ltr` presets green for phases —
and the screen-specific parts of each were ported before this session: the screen
QP bounds and `CComplexityAnalysisScreen`'s feed into rate control at P10.2.C5
and C7 (refereed there by first-P-frame `iFrameComplexity` on five `rc=1` rows),
the lossless-LTR reference strategy and `UpdateBlockStatic` at P10.1/P10.2. What
was missing was only the dispatch, and the dispatch is not per-axis. The two
halves met.

The scope rule stands where it is: `scc` is **not** added to `gates.sh`'s family
list here (D-scc-16), so P10.4's first item is that promotion and its cost, not a
defect hunt.

## F332 — the 16x16 feature-search family is unreachable in upstream's own configuration (P10.3, D7)

Two ported bodies read **zero** on every screen row at D7's tag census while their
8x8 twins read 1304/4185/565 and 57/57/59: `sum_of_16x16_single_block`
(`pfCalculateSingleBlockFeature[1]`) and `SumOf16x16BlockOfFrame_c`
(`pfCalculateBlockFeatureOfFrame[1]`).

One pair of compile-time constants explains both. `encoder_ext.cpp:1030-1031`:

    const int32_t kiMe16x16 = ME_DIA_CROSS;        // 0x03 — no ME_FME bit
    const int32_t kiMe8x8   = ME_DIA_CROSS_FME;    // 0x07 — has it

- `PreprocessSliceCoding` therefore calls `SetMeMethod(ME_DIA_CROSS_FME, ..)` on
  `pfSearchMethod[BLOCK_8x8]` and `SetMeMethod(ME_DIA_CROSS, ..)` on
  `[BLOCK_16x16]`, so `WelsDiamondCrossFeatureSearch` — the only caller of
  `SetFeatureSearchIn`, which is the only reader of the single-block slot — runs
  only for `uiBlockSize == BLOCK_8x8`, and that selects index 0.
- The same `kiMe8x8` reaches `AllocPicture` as `bIsBlock8x8 = (kiMe8x8FME ==
  ME_FME)`, always true, so every `SScreenBlockFeatureStorage` carries
  `iIs16x16 == 0` and `CalculateFeatureOfBlock` indexes the frame builder at 0.
  (`PreprocessSliceCoding` also refuses to install the feature search at all
  unless `iIs16x16 == 0`, which is a third, redundant lock on the same value.)

So this is not port debt. Both bodies are faithful and both are kept — deleting
them would delete the reference, not dead port code — and neither carries a
`SCREEN_CONTENT(dormant)` tag any more, because the tag meant "Phase 10 has not
reached this yet" and Phase 10 has. `sum_of_16x16_single_block`'s doc states the
condition instead; the tag count reads zero, which is the phase's exit.

The reachable way in would be upstream changing `kiMe16x16` to carry `ME_FME`.
Nothing in this port would need to change if it did, but nothing here has ever
exercised that path either, so it would arrive untested.

## F333 — `PerformFMEPreprocess` runs on every screen P frame: `bRefBlockFeatureCalculated` never survives a picture's recycle (P10.3, D7)

D5's counters read `PerformFMEPreprocess` 57 and `UpdateFMESwitch` 59 on
`scc_text_320x192_k3`, and D5's commit message read the gap as the
`!pScreenBlockFeatureStorage->bRefBlockFeatureCalculated` guard (`:2744`) skipping
two rebuilds. **That reading is wrong**, and D7's finer census says why: on all
three screen rows `CalcFMESwitchFlag` and `PerformFMEPreprocess` are *equal* —
57/57, 57/57, 59/59 — one of each per screen P frame, so the guard never fires.

Two separate facts were being conflated.

1. **The features are always stale.** `SetUnref` clears
   `bRefBlockFeatureCalculated` when a reference picture is returned to the pool
   (`picture.rs:549`, and the C++ the same), and the single-reference screen
   configuration recycles the picture every frame. Nothing else sets the flag but
   `PerformFMEPreprocess` itself. So the "already built" arm is dead in this
   configuration — the guard exists for a reference held across frames, which the
   `min` tier never produces. Note the asymmetry upstream leaves and the port
   keeps: `uiSadCostThreshold` is **not** reset by `SetUnref`, only the flag is,
   so a storage carries the previous reference's thresholds until the next
   `PerformFMEPreprocess` overwrites them — harmless only because the rebuild
   always happens.
2. **`UpdateFMESwitch`'s two extra entries are at the other end of the frame.**
   `PreprocessSliceCoding` writes `pfUpdateFMESwitch` only inside its P-slice arm;
   the I-slice arm resets the preparation's two fields and leaves the slot alone.
   The post-join call (`encoder_ext.cpp:3891-3897`) is unconditional. So on a
   forced IDR the *previous* P frame's choice runs, and `k3`'s two scene cuts are
   exactly the two extra entries (57 P + 2 forced IDR = 59; the first IDR still
   holds `WelsInitMeFunc`'s `UpdateFMESwitchNull`). On `scc_text_160x96_k1`, which
   has no cuts, all three counters read 59.

Both are upstream's, both are ported as-is, and the bytes agree — which is the
point of recording it: the counters looked like a guard doing work and were
measuring a carry-over instead.

