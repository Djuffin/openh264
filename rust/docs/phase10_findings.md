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
