# The port census — what the reference has and this port does not

*Phase 8b session A, T8b.A2; **re-measured at session C's close (T8b.C5)**.
Regenerate with `rust/tools/port_census.py --classify` and
`rust/tools/find_stub_bodies.py`; the classification lives in
`rust/tools/port_census_classification.txt`, not here.*

## 1. Why this file exists

F80 (`IncreasePicBuff`/`DecreasePicBuff`, never ported), the three parameter-set
listing strategies, the denoise and downsample plugins, `GetVclNalTemporalId`'s empty
body and `uiCurIdrPicId`'s missing writer were each found **by accident, one at a
time, over four phases**. Nothing in the tree asked "what is not there at all":

* `find_stub_bodies.py` diffs *call sets*, so it can only ask about functions the
  port already has — and a C++ body that only **assigns** is a zero-call body, equal
  to a Rust `{}`. That is S46's own blind spot, and `GetVclNalTemporalId` sat in it.
* Its `CPP_DIRS` omitted `codec/decoder/plus/src` and `codec/encoder/plus/src`, so
  the entire public entry-point layer — `GetOption`, `SetOption`, `DecodeParser`,
  `ForceIntraFrame` — had never been compared against anything.

Both holes are closed (§4). The list below is the rest of this phase's inventory, and
it is what makes the exit claim honest.

## 2. The numbers

`port_census.py`, **re-run at T8b.C5** (T8b.B4's numbers are in the second column):

```
1351 C++ definitions (name × file) under codec/{decoder,encoder,processing,common}/**/src,
     SIMD directories and SIMD-suffixed names excluded
 271 with no same-name Rust definition (was 283), of which
     156 dead     — the reference cannot reach it in the configuration this port ships   (was 154)
      96 renamed  — ported under another name, or folded into a neighbour                (was  96)
      19 missing  — a real gap                                                           (was  33)
       0 unclassified
```

**33 → 19.** Session C ported the denoise and downsample kernels (T8b.C1/T8b.C2) and
`IncreasePicBuff`/`DecreasePicBuff` (T8b.C3), which is twelve of the fourteen; the
other two moved to `dead` and did **not** move because nobody wanted to port them.
`GeneralBilinearFastDownsampler_c` and `GeneralBilinearDownsamplerWrap` are
unreachable on this target, measured rather than assumed — the aarch64 dispatch table
rebinds general-ratio *luma* to the accurate downsampler because there is no NEON fast
kernel, so nothing can call the fast one, and `rust/tools/vp_kernel_probe/` shows the
two are genuinely different functions. Porting it would add a kernel with no caller
and no referee. See **F97**.

`find_stub_bodies.py`, at `b9599b96`: **241 flagged** of 1898 Rust functions (956 have
a C++ counterpart) by the call-set diff, and **9** by the new empty-body rule.

The 19 `missing`, by owner:

| owner | rows | what |
|---|---|---|
| **8b.B** | **0** | ported at T8b.B2/T8b.B3 — see below |
| **8b.C** | **0** | ported at T8b.C1/T8b.C2; the two residual kernels are `dead` (F97) |
| **F80** | **0** | ported at T8b.C3 |
| **Phase 10** | **0** | ported or reclassified 2026-09-02 — see below |

**Every remaining row was Phase 10's.** Phase 8b's own inventory is empty.

> **2026-09-02, Phase 10's close — this row is 0 and the census reads `0 missing`.**
> Of the 19: **fifteen were ported** under their C++ names across P10.1–P10.3 —
> scroll detection (4 + 2) and the screen complexity analysis (2) as the plugins at
> P10.2, the feature-search storage (5) at P10.1.B3/P10.3.D1, the VAA screen buffers
> (2) at P10.1.B3. The remaining **four are `imagerotate`'s, reclassified `dead` at
> P10.2** with the evidence line: `CImageRotating` is registered in upstream's
> plugin table under `METHOD_IMAGE_ROTATE`, which no encoder path requests — the
> only `Set(METHOD_IMAGE_ROTATE, ...)` in `codec/` is in the standalone processing
> console demo, not in `encoder_ext.cpp`, so porting it would add a plugin with no
> caller and no referee. Same rule as F97's two residual kernels.
>
> `port_census.py --classify` now reads
> **`0 missing, 101 renamed, 161 dead, 53 unclassified`** — the first `0 missing` in
> the project's history. The 53 `unclassified` all predate Phase 10; their
> disposition is in `encoder_port_handoff.md` §5.

**Session B's nine rows are gone, and one of them was never 8b.B's.**

* The `paraset_strategy.cpp` family and `WriteSavcParaset_Listing` are ported
  (T8b.B3). Four names stay in the census as **renamed**, not missing:
  `LoadPreviousSps`, `LoadPreviousPps`, `CheckPpsGenerating` and `SpsReset` are
  per-class C++ methods that collapse onto one method each on the merged
  `CWelsParametersetIdStrategyObj`, which is the shape T4b.2a chose and T8b.B3
  extended.
* `CWelsDecoder::ParseAccessUnit` is **dead, not missing** — session A filed it under
  8b.B as "parse-only's AU walk", and its only caller is `welsDecoderExt.cpp:1384`,
  inside `ThreadDecodeFrameInternal`, the threaded decoder D3 deletes. Parse-only's
  real work went in at T8b.B2 and never touches it. Reclassified by rule in
  `port_census_classification.txt`, not by hand here.

Nothing in the `missing` column is unowned. **8b.A owns none of it** — the session's
own work (the decoder option arms, the two feeders, LTR) was all *present-but-wrong*,
which is the other tool's column.

### The top of the `missing` list, by size — **Phase 10's, all of it**
(the six 8b.C and F80 rows that stood here — `DecreasePicBuff` at 50 statements,
`GeneralBilinearAccurateDownsampler_c` at 42, `IncreasePicBuff` at 36,
`BilateralLumaFilter8_c` at 30 and the two kernel files — are ported;
`GeneralBilinearFastDownsampler_c` at 49 is reclassified `dead`)

| C++ | stmts / lines | owner |
|---|---|---|
| `ScrollDetectionCore` (`ScrollDetectionFuncs.cpp:110`) | 57 / 87 | Phase 10 |
| `CComplexityAnalysisScreen::GomComplexityAnalysisInter` (`ComplexityAnalysis.cpp:413`) | 46 / 81 | Phase 10 |
| `CComplexityAnalysisScreen::GomComplexityAnalysisIntra` (`ComplexityAnalysis.cpp:357`) | 32 / 53 | Phase 10 |
| `CheckLine` (`ScrollDetectionFuncs.cpp:37`) | 21 / 33 | Phase 10 |
| `RequestScreenBlockFeatureStorage` (`svc_motion_estimate.cpp:683`) | 20 / 44 | Phase 10 |

The full list, with the evidence for every row, is the tool's output:

```bash
python3 rust/tools/port_census.py --classify
```

## 3. What `dead` rests on

156 rows, and they are not a shrug — each has a rule with evidence in
`port_census_classification.txt`. Four families, counted by which marker the rule's
evidence carries (threaded first, then debug/trace, then the runtime, then the rest):
**101 threaded · 23 runtime and allocator · 19 SIMD dispatch · 12 debug and trace**
(SIMD dispatch gained the two general-ratio downsample rows at T8b.C5, F97).

* **The threaded detour, 101 rows.** `CWelsThreadPool` and the task classes were
  deleted at T7.B4 (`common/mod.rs:10–13`, `encoder/mod.rs:42–43`): the encoder forks
  with `std::thread::scope` and joins on the scope, so there is no object for them to
  be (D-mt-1). The decoder is single-threaded by D3, so `wels_decoder_thread.cpp`,
  `CWelsDecoder::{Open,Close}DecoderThreads`, `ThreadDecodeFrameInternal` and
  `ThreadResetDecoder` have no reachable caller.
  * Included here: **the 18 `MBPad*_c` / `PadMB*_c` kernels** in `expand_pic.cpp`.
    Their only caller is `decode_slice.cpp:1731`, inside `WelsDecodeAndConstructSlice`,
    and `decoder_core.cpp:2727` reaches *that* only under `iThreadCount > 1`. Worth
    stating plainly because it looks like a live decode path and is not.
* **The C runtime and the allocator, 23 rows** (plus the trace object, counted
  with debug below).
  `crt_util_safe_x.cpp`, `memory_align.cpp`, `processing/src/common/memory.cpp`,
  `welsCodecTrace.cpp`, `DllEntryPoint`. Rust's std and `Vec`/`Box` are the port's.
* **Debug, statistics and trace, 12 rows.** Every call site is behind
  `#if defined(MB_TYPES_CHECK)` (`WelsCountMbType`), `#if defined(STAT_OUTPUT)`
  (`StatOverallEncodingExt`), `#ifdef ENABLE_FRAME_DUMP` (`DumpRecFrame`,
  `DumpSrcPicture`), or is a bare `WelsLog` block.
* **SIMD selection and CPU probing, 17 rows.** `VerticalFullSearchUsingSSE41` and
  `HorizontalFullSearchUsingSSE41` live in a scalar file behind `#if defined(X86_ASM)`;
  the `Init*Funcs` tables pick between kernels this port does not have, because the
  plan keeps the port scalar and the diffharness pins it against the scalar C++.

## 4. The instruments, and the two holes closed in them

**`find_stub_bodies.py`** gained:

1. `codec/decoder/plus/src` and `codec/encoder/plus/src` in `CPP_DIRS`.
2. **The alias table**, shared with `port_census.py` so the two cannot disagree.
   Without it the C++ `CWelsDecoder::DecodeParser` was compared against a *vtable
   trampoline* that happens to share the name and came out clean, while the real
   thunk `decoder_decode_parser_c` returns `dsErrorFree` and writes nothing.
3. **The empty-body rule** — a Rust body that is empty or one bare literal, opposite
   a C++ body of ≥ 2 statements, reported in its own section.
4. A one-line fix that mattered more than the rule: `if len(body) > len(bodies.get(…))`
   meant **an empty body was never stored at all** (`len("") > len("")` is false), so
   `GetVclNalTemporalId` vanished from the map before any rule could see it. The
   instrument had the exact blind spot it exists to find.

The 9 empty/constant bodies it now reports:

| Rust | body | C++ |
|---|---|---|
| `GetVclNalTemporalId` | *(empty)* | `decoder.cpp:716`, 5 stmts — **fixed in T8b.A3** |
| ~~`BilateralDenoising`~~ | *(was empty)* | **ported at T8b.C1** — `wels_preprocess.rs:1583` runs the filters in place |
| `UpdatePpsList` | *(empty)* | `paraset_strategy.cpp`, 15 stmts — 8b.B |
| `LoadPrevious` | *(empty)* | `paraset_strategy.cpp`, 3 stmts — 8b.B |
| `UpdateParaSetNum` | *(empty)* | `paraset_strategy.cpp`, 2 stmts — 8b.B |
| `CheckParamCompatibility` | `true` | `paraset_strategy.cpp`, 4 stmts — 8b.B |
| `TraceParamInfo` | *(empty)* | `welsEncoderExt.cpp`, 44 stmts — trace-only (`dead`) |
| `LogStatistics` | *(empty)* | `welsEncoderExt.cpp`, 4 stmts — trace-only (`dead`) |
| `GetCPUCount` | `1` | `wels_decoder_thread.cpp`, 4 stmts — single-threaded decoder (D3) |

**`port_census.py`** is new. It is deliberately dumb — a by-name match, with two
mechanical renames applied as a fallback (the scalar-kernel `_c` suffix and the
`Wels`/`CWels` prefix with CamelCase, e.g. `WelsI8x8LumaPredDDR_c` →
`i8x8_luma_pred_ddr`; 29 rows match only that way and each was spot-read). All
judgement lives in `port_census_classification.txt`, and `--classify` reports what no
rule covers. **That is the point of it**: the next unported function shows up as
`UNCLASSIFIED` on the next run, not as an accident four phases later.

### Red-proof of the instrument

The brief named six things the census had to contain. All six:

| required | where it shows up |
|---|---|
| `IncreasePicBuff` | `--classify` → missing, `decoder.cpp:107`, 36 stmts |
| `DecreasePicBuff` | `--classify` → missing, `decoder.cpp:170`, 50 stmts |
| `GetVclNalTemporalId` (empty) | `find_stub_bodies.py` → empty-body section |
| `DecodeParser`'s body | `find_stub_bodies.py` → flagged, "not called on the Rust side: CheckBsBuffer, ResetDecStatNums, ResetDecoder, WelsDecodeBs, WelsFflush, WelsFwrite, WelsTime" |
| the three listing-strategy classes' methods | `--classify` → missing (`CWelsParametersetSpsListing`, `CWelsParametersetSpsPpsListing`, `SpsReset`, `LoadPreviousSps`, `LoadPreviousPps`, `CheckPpsGenerating`, `FindExistingPps`) **and** `find_stub_bodies.py`'s empty section (`UpdatePpsList`, `UpdateParaSetNum`, `LoadPrevious`, `CheckParamCompatibility`) |
| the denoise / downsample plugins | `--classify` → missing, 12 rows |

## 5. Known limitations

* **F85 — same-named methods on two classes collapse.** `find_stub_bodies.py` keys
  the C++ map by bare name and keeps the longest body, so
  `CWelsDecoder::GetOption` (53 stmts) is invisible behind
  `CWelsH264SVCEncoder::GetOption` (230). Anything the decoder's version calls and
  the encoder's does not is un-diffable by that tool. `port_census.py` does not have
  this problem (it keys by name × file and prints the class), but it only answers
  "present", not "equivalent".
* **A name present on both sides is never questioned.** That is the *other* tool's
  job, and the two are meant to be read together. The 21 decoder-option gtest rows
  this session fixed were all in that gap: `decoder_get_opt_c` existed.
* **F84 — dead code in the port, found while classifying.** The port's own
  `WelsDecodeAndConstructSlice` (`decode_slice.rs:5444`) and `WelsDeblockingFilterMB`
  (`deblocking.rs:2295`) have **zero callers**: they are the threaded-decoder path's,
  transliterated and then orphaned. S18 candidates for a later session; not deleted
  here because this session's scope is parity, not hygiene.
