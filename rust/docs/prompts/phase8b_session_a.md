# Phase 8b session A — the parity plumbing, the gate, and the census

*Execution prompt. Phase 8 session C continues as this session: the harness, the
gtest driver and the `--nodelay` referees are warm. Start commit **`cd1bc977`**
(code identical to `863eaec7`). Commits are named `T8b.A<n>`. Charter:
`rust/docs/prompts/phase8b.md`; the plan's Phase 8b section and D-prio-1 (§7.4),
S47/S48 (§7.6) are the governing text.*

## Context

Correctness and feature parity come before safety and performance (D-prio-1). The
measurement is upstream's own `test/api` suite against the Rust cdylib: **155/199**
at the start commit, and the 44 failures decompose by *assertion site* into seven
features (charter §2). This session takes the two cheapest families — the decoder
option arms (21 rows) and LTR/`ForceIntraFrame` (3) — builds the instruments the
phase runs on (the gtest gate with a named allowlist; the widened stub census and a
by-name completeness census), makes the two silent unported paths refuse (S48),
measures F80's reachability, and installs the F77 instrument. Expected tally at the
close: **≥ 179/199**, with every remaining failure named in the allowlist with an
owner.

## Hard rules

1. **Every fix lands with a referee that reads the reference's answer**, in the tree,
   measured red before and green after: an in-tree test pinning the C++'s values (via
   `rust/tools/ecref/ecref`, `cxx_enc`, or the reference test's own logic re-stated),
   plus the gtest rows flipping and their allowlist rows deleted in the same commit.
   `gates.sh commit` green per commit; `exit` once at the close, unscoped Miri.
2. **Tally by assertion site, never by fixture name** (S47). Before claiming what a
   row needs, read the failing assertion and the test lines around it.
3. **S48**: anything unported that this session does not port must refuse at the entry
   point with a test pinning the error code. No silent success survives this session.
4. **Anchors, not surfaces**: every count and line number below was read at
   `863eaec7`; re-grep before acting and trust the tree. State the grep and the unit
   beside every number you report.
5. **Stop and report** (do not decide) on: a divergence where the port looks more
   correct than the reference (D-poc-1's class); any fix that needs a raw signature
   *and* a borrow the context tree cannot express (that is Phase 9's F66 class).
6. No perf work (D-gate-1): measure one 7-pair span at the close, no mid-session
   pairs. No conversions of the `port-raw(Phase 9)` tree; a new raw signature forced
   by a raw neighbour is tagged `port-raw(Phase 9)`; `unsafe_ratchet.sh check` green
   per commit, rebaselined only with the reason in the message.
7. "Not ported" in a comment is a claim: grep the named function before believing it
   (`encoder_ext.rs:3022` still says MT is unported; it is not).
8. Breadcrumbs to the log as you go (S31); one span; drop-from-the-end (steps 7, 6 are
   the first to go; 5 shrinks before it drops).

## Current state — verified facts (at `863eaec7`)

**The gtest driver.** `rust/tools/abi_harness/gtest_stretch.sh` builds
`cargo build --release`, links `test/api/*.o` + `libgtest.a` against the cdylib (with
`-rpath` to `target/release`) into `tools/abi_harness/out/gtest/api_gtest_rust` and
against `libopenh264.a` into `api_gtest_cxx`, runs both, tallies from the gtest
summary lines, and prints the Rust failures **by fixture name** (the `awk` at the end
— this is the reading S47 retires). It has **no filter** and is **not a gate**
(header comment). After `cargo build --release` the existing `api_gtest_rust` picks
up the new dylib without relinking (rpath); relink only when the export list moves.
Prerequisite objects exist (`make -j8 libraries binaries` was run).

**The 44 rows by assertion site** (from `out/gtest/rust.log`, 21:47):

| assertion site | rows | tests |
|---|---|---|
| `decode_api_test.cpp:45` | 3 | `DecoderVclNal/0..2` — `GetOption(DECODER_OPTION_VCL_NAL)` must read `FEEDBACK_UNKNOWN_NAL` after the first call and `FEEDBACK_VCL_NAL` after `DecodeFrame2(NULL,0)` |
| `:81` | 3 | `GetOptionFramenum` — `FRAME_NUM` −1 after the first call, then the encoder's frame num |
| `:132` | 3 | `GetOptionIDR` — `IDR_PIC_ID` equals the encoder's on both calls |
| `:204` | 3 | `GetOptionIsRefPic` — `IS_REF_PIC` −1 before any decode |
| `:348`, `:404` | 6 | `GetOptionTid_*` — `TEMPORAL_ID` −1 after the first call, the layer's `uiTemporalId` after reconstruction |
| `decoder_ec_test.cpp:701/789` | 2 | `Engine_SVC_Switch_I/P` — the same `TEMPORAL_ID` read |
| `encode_options_test.cpp:1990` | 1 | `ProfileLevelSetting` — `DECODER_OPTION_PROFILE` equals the encoder's profile |
| `ltr_test.cpp:39` | 3 | `GetOptionLTR_ALLIDR/0..2` — LTR on (`iLTRRefNum=1`, marking period 2): every frame after `ForceIntraFrame(true)` must report `videoFrameTypeIDR` |
| `encode_options_test.cpp:556/656/705/772/844/911` | 6 | listing strategies — **session B** |
| `decode_api_test.cpp:923/1001/1048` | 3 | parse-only — **session B** |
| `HashFunctions.h:31` | 3 | `EncoderOutputTest/4,5,7` — denoise / 2 layers / 4 layers — **session C** (this session: refuse, S48) |
| `BaseEncoderTest.cpp:92` ×5, `encode_options_test.cpp:2195`, `encoder_test.cpp:323` | 7 | screen content — **Phase 10** |
| `HashFunctions.h:20` | 1 | POC tiebreak — **D-poc-1** |

**The decoder option arms.** `decoder_get_opt_c` (`src/api/codec_api.rs:3375`) handles
`NUM_OF_FRAMES_REMAINING_IN_BUFFER`, `END_OF_STREAM`, `ERROR_CON_IDC`,
`GET_STATISTICS`, then `_ => {}` and returns success — nothing written.
`decoder_set_opt_c` (`:3299`) handles `END_OF_STREAM`, `ERROR_CON_IDC`,
`GET_STATISTICS`, `TRACE_LEVEL`, `TRACE_CALLBACK`, `TRACE_CALLBACK_CONTEXT`. The
safe core's accessors are `Decoder::frames_remaining/end_of_stream/error_concealment/
statistics` (`:2225–2267`). Upstream: `CWelsDecoder::GetOption`
(`codec/decoder/plus/src/welsDecoderExt.cpp:584–695`) has **17** arms — `DATAFORMAT`,
`END_OF_STREAM`, `VCL_NAL`, `TEMPORAL_ID`, `FRAME_NUM`, `IDR_PIC_ID`,
`LTR_MARKING_FLAG`, `LTR_MARKED_FRAME_NUM`, `ERROR_CON_IDC`, `GET_STATISTICS`,
`STATISTICS_LOG_INTERVAL`, `GET_SAR_INFO`, `PROFILE`, `LEVEL`, `IS_REF_PIC`,
`NUM_OF_FRAMES_REMAINING_IN_BUFFER`, `NUM_OF_THREADS`; `SetOption` (`:479–584`) has
**10** — `DATAFORMAT`, `END_OF_STREAM`, `ERROR_CON_IDC` (with the parse-only
restriction), `TRACE_LEVEL`, `TRACE_CALLBACK`, `TRACE_CALLBACK_CONTEXT`,
`GET_STATISTICS` (get-only → error), `GET_SAR_INFO` (get-only), `STATISTICS_LOG_INTERVAL`,
`NUM_OF_THREADS`. Read them whole; port every arm and every return code.

**Two feeders are stubs, so arms alone will not flip the rows:**
* `GetVclNalTemporalId` — `src/decoder/decoder_core.rs:1061` is `pub fn
  GetVclNalTemporalId(pCtx: &mut SWelsDecoderContext) {}`, called at `:3647`.
  Upstream `codec/decoder/core/src/decoder.cpp:716–724` writes
  `iFeedbackVclNalInAu = FEEDBACK_VCL_NAL`, `iFeedbackTidInAu`, `iFeedbackNalRefIdc`
  from the access unit's last VCL NAL header. The port's fields exist
  (`decoder_context.rs:1864–1866`); the only writer is the reset
  `iFeedbackNalRefIdc = -1` at `decoder_core.rs:2056`.
* `uiCurIdrPicId` — declared `decoder_context.rs:1820`, reset at `:2063`, **never
  written**; upstream `decoder_core.cpp:1061` writes it from the slice header under
  `LONG_TERM_REF`, which `decoder_context.h:67` defines.
* For every other arm's source field (`iFrameNum` — written at `decoder_core.rs:4014`;
  `bCurAuContainLtrMarkSeFlag`/`iFrameNumOfAuMarkedLtr` — `manage_dec_ref.rs:843/883`;
  `pSps` profile/level/VUI; reordering count), confirm the writer exists at the
  reference's point before porting the arm. The signature of a missing one is F77's:
  declared, reset, never set.
* `GetPrevFrameNum` (`decoder_core.rs:1064`, returns 0) is the multi-threaded detour's
  helper; the port's one call site reads `pLastDecPicInfo.iPrevFrameNum` directly
  (`:4356`), as upstream's single-threaded read does. **Delete it (S18)**, do not fix
  it.

**Why `find_stub_bodies.py` missed them.** `rust/tools/find_stub_bodies.py` diffs the
*call sets* of C++ and Rust bodies (header docstring); a C++ body that only assigns
is a zero-call body and a Rust `{}` calls zero things — equal, not flagged. And its
`CPP_DIRS` (line ~30) are `encoder/core/src`, `decoder/core/src`, `common/src`,
`processing/src` — **no `codec/decoder/plus/src`, no `codec/encoder/plus/src`**, so
`GetOption`/`SetOption`/`DecodeParser`/`ForceIntraFrame` were never compared. It
reports 223 flagged of 1887 functions today (914 with a C++ counterpart).

**LTR / `ForceIntraFrame`.** Port `wels_encoder_ext.rs:1940` matches upstream
`welsEncoderExt.cpp:491–505` (`if bIDR { ForceCodingIDR(ctx, iLayerId) }`). The
failing assertion is `info.eFrameType == videoFrameTypeIDR` on the *next* encoded
frame with LTR enabled. Candidates, in reading order: `ForceCodingIDR`
(`wels_encoder_ext.rs:629` vs `encoder_ext.cpp`), the LTR marking/recovery path that
can demote an IDR request when `bEnableLongTermReference` is on, and how
`eFrameType` is filled into `SFrameBSInfo` in the port's `EncodeFrame`. Unknown
until read; the test's own loop (`ltr_test.cpp:28–42`) is the covering test to
re-state in-tree.

**The silent pair (S48).** `BilateralDenoising` (`wels_preprocess.rs:1590`) is an
empty body behind `bEnableDenoise`; `DownsamplePadding` (`:1596`) returns
`RET_NOTSUPPORTED` at `:1647` when sizes differ and **both callers drop the return**
(`:1240`, `:1327`). `processing/mod.rs:29–39` lists the five untranslated methods.
`EncoderOutputTest` rows 4/5/7 prove the consequence: the encode *succeeds* and the
bytes differ. Upstream sizes: denoise 250 lines, downsample 594
(`codec/processing/src/{denoise,downsample}/*.cpp`).

**F80.** `IncreasePicBuff`/`DecreasePicBuff` (`decoder.cpp:495–508` and the two
functions) — `grep -rn 'IncreasePicBuff\|DecreasePicBuff' src/` is empty; the
third arm of `WelsRequestMem` is absent. No `res/` asset is known to change
`num_ref_frames` at constant resolution. `rust/tools/ecref/ecref <stream> <bytes>
[--frames] [--nodelay]` is the C++ referee (`build.sh`, `compare_all.sh`).

**F77 instrument.** `safe/mb_grid.rs:272–282`: `get_mut`/`at` index `self.data[mb_xy]`
bare; `MbDims` is `mb_width`/`mb_height`. The F77 panic read `mb_grid.rs:277` and
nothing else.

**Gate wiring pattern.** `rust/tools/gates.sh:461–485` runs `abi_exports.sh` and
`abi_harness/run.sh` at `exit`, requiring both `PIPESTATUS[0] == 0` **and** a verdict
line (`exports: n/n`, `TALLY`) — a script that dies before its verdict fails the gate.

## Steps

### Step 0 — open
Record HEAD; `gates.sh commit`; run `gtest_stretch.sh` once and confirm **155/199**
(this is the before-number). Log the start.

### Step 1 — the gtest gate and filter
**Goal:** the tally becomes a ratchet that can only move by editing a named list.
**Do:** add `tools/abi_harness/gtest_known_failures.txt` — one row per failing test,
`test-name | owner | reason` (parametrized rows spelled out, e.g.
`EncodeDecodeTestAPIBase/EncodeDecodeTestAPI.DecoderVclNal/0`), initial content the
44 with owners from the table above. `gtest_stretch.sh --check`: run the Rust binary,
fail (rc 1) if any failing test is not in the list **or any listed test passes**;
print `gtest: <pass>/<total>, allowlist <n>` as the verdict line. `--filter=<pat>`
passes `--gtest_filter` through. Replace the fixture-name `awk` with a by-assertion-site
tally (first `*.cpp:NNN: Failure` line per failed test). Wire `--check` into
`gates.sh exit` in the `:461–485` pattern. **Red-proof both ways** by exit code: delete
one row → rc 1 naming the unlisted failure; add a passing test's name → rc 1 naming the
stale row; restore → rc 0. Commit `T8b.A1`.
**Accept:** `gates.sh exit`'s gate list grows by one; the verdict line prints; both
red-proofs recorded in the log with their rc.

### Step 2 — the census instruments
**Goal:** the unported and stubbed functions become a list, not a series of accidents.
**Do:** (a) `find_stub_bodies.py`: add `codec/decoder/plus/src` and
`codec/encoder/plus/src` to `CPP_DIRS` and the matching Rust files
(`api/codec_api.rs`, `encoder/wels_encoder_ext.rs`); add an **empty/constant-body
rule** — flag a Rust function whose body is empty or a single literal/`return` when
the C++ body has ≥ 2 statements — reported in its own section. (b) New
`tools/port_census.py`: every C++ function definition under
`codec/{decoder,encoder,processing,common}/**/src` (by name, SIMD/arch directories
excluded) checked for a same-name definition under `src/` (a small alias table for the
port's known renames, e.g. `SParserBsInfo`→`ParseOnlyBsBuffers`); output a table per
C++ file: *missing* / *present*. (c) Run both at the start commit and write
`rust/docs/phase8b_port_census.md`: the missing + flagged-empty list, each row
classified by **reading the C++** — `dead` (no caller / SIMD-only / `_DEBUG`-only /
threaded-decoder-only) with the evidence, `renamed` with the port name, or `missing`
with the reference lines and a size. **Red-proof of the instrument:** the list must
contain `IncreasePicBuff`, `DecreasePicBuff`, `GetVclNalTemporalId` (empty at the
start commit), `DecodeParser`'s body, the three listing-strategy classes' methods,
and the denoise/downsample plugins; if any is absent the instrument is wrong, fix it
before trusting anything else it says. Commit `T8b.A2`.
**Accept:** the census file exists with every row classified; the `missing` count and
the `dead` count stated with the grep that produced them; the red-proof list all
present.

### Step 3 — the decoder option arms (21 rows)
**Goal:** `GetOption`/`SetOption` return the reference's values and codes for every
option id.
**Do:** port `CWelsDecoder::GetOption` and `SetOption` whole into the thunks (or the
safe core, with the thunks translating) — every arm, every error code, the get-only
arms rejecting on set, the parse-only restriction on `ERROR_CON_IDC`. Port
`GetVclNalTemporalId`'s body (`decoder.cpp:716–724`); write `uiCurIdrPicId` at the
slice-header point (`decoder_core.cpp:1061`); for every other arm, confirm the
feeder's writer exists (fact list above) and add the missing ones at the reference's
points; delete `GetPrevFrameNum`. **Referee:** `ecref --options` — after every
`DecodeFrame2` call print the eleven get-able scalar options (`VCL_NAL`,
`TEMPORAL_ID`, `FRAME_NUM`, `IDR_PIC_ID`, `LTR_MARKING_FLAG`, `LTR_MARKED_FRAME_NUM`,
`ERROR_CON_IDC`, `PROFILE`, `LEVEL`, `IS_REF_PIC`, `NUM_OF_FRAMES_REMAINING_IN_BUFFER`)
— and `tests/decoder_options_parity_test.rs` pinning those per-call values for at
least five assets (include `Error_I_P.264` and one LTR stream) from the C++'s output,
measured red before the port. Then `gtest_stretch.sh --filter='*GetOption*:*DecoderVclNal*:*Engine_SVC_Switch*:*ProfileLevel*'`.
Commit `T8b.A3` (split by family if the diff is large: feeders, get arms, set arms).
**Accept:** the 21 rows pass; their allowlist rows deleted; the parity test green
with the C++'s values; corpus (`compare_all.sh`) and conformance unmoved.

### Step 4 — LTR with `ForceIntraFrame` (3 rows)
**Goal:** with LTR on, `ForceIntraFrame(true)` yields an IDR on the next frame, as the
reference does.
**Do:** re-state `ltr_test.cpp:20–45`'s loop as `tests/encoder_force_idr_ltr_test.rs`
through the C API (frame types per call; the reference's expectation is every frame
IDR); run it — red. Read, in order: `ForceCodingIDR` port vs `encoder_ext.cpp`; the
LTR marking/recovery code that touches the IDR request; `eFrameType` filling in
`EncodeFrame`. Fix at the cause, statement-for-statement with the reference. If the
cause turns out to be in the port's `eFrameType` reporting only, say so — the byte
stream may already be right and the diffharness would not have seen a reporting gap.
Commit `T8b.A4`.
**Accept:** 3 rows pass; the in-tree test green; the `ltr` sweep preset unchanged.

### Step 5 — S48 for the silent pair
**Goal:** until session C ports them, asking for denoise or a downsampled layer gets
an error, not silent output.
**Do:** at parameter validation (`ParamValidationExt`'s port in `wels_encoder_ext.rs`,
where `ENC_RETURN_UNSUPPORTED_PARA` is already returned for other cases) reject
`bEnableDenoise` and any spatial layer whose size differs from the source, returning
the code `InitializeExt` maps to `cmInitParaError`/`cmUnsupportedData` (state which
and why at the site, with a `// S48, until Phase 8b session C` marker); confirm
`GetDefaultParams` defaults (`bEnableDenoise = false`, one layer) keep every gate and
the `def` preset untouched — grep the harness sources. Covering test: asks for each,
asserts the code. Re-reason the three allowlist rows (`refuses at init; port in 8b.C`).
Check `METHOD_IMAGE_ROTATE`'s reachability while there (who sets it?); if only
screen content can reach it, note it for Phase 10. Commit `T8b.A5`.
**Accept:** the two requests return an error; tests pin it; sweeps 369/369 unmoved;
gtest rows 4/5/7 fail at `BaseEncoderTest.cpp:92` now (init), listed as such.

### Step 6 — F80 reachability
**Goal:** know whether `WelsRequestMem`'s third arm is reachable, and port it if it
is.
**Do:** add `ecref --sps` (per activated SPS: dims, `num_ref_frames`) and scan every
`res/*.264`; if none changes `num_ref_frames` at constant dims, make one with the
*reference* encoder (`cxx_enc` or a 40-line driver against `libopenh264.a`:
`ENCODER_OPTION_NUMBER_REF` changed between two forced IDRs). If the arm is reachable:
port `IncreasePicBuff`/`DecreasePicBuff` (`decoder.cpp:495–508` + the two functions)
and add the asset to the corpus (`compare_all.sh` TABLES + a `stream_case!`), codes
and output gated against the C++. If no stream the reference can produce reaches it,
say so at the site and close F80 as *unreachable-by-construction*, with the evidence.
Commit `T8b.A6`.
**Accept:** a verdict with evidence either way; if ported, corpus rows agreeing.

### Step 7 — the F77 instrument
**Goal:** a grid index panic names its caller and both dimensions.
**Do:** `#[track_caller]` on `MbGrid::get/get_mut/at/at_mut` (and the neighbour
helpers) with an explicit `assert!(mb_xy < len, "mb_xy {} >= {} (grid {}x{})", …)`;
a `#[should_panic(expected = "grid ")]` test. No measurement mid-session (D-gate-1);
the bounds check already existed, the assert replaces it. Commit `T8b.A7`.

### Step 8 — close
`gates.sh exit` unscoped (Miri once, whole library); `gtest_stretch.sh --check`;
the span: 7 pairs both benches against the start commit (+ a 3-pair null); the log
entry; update `phase8b.md` §5's session A row with what landed; report.

## Do not touch

| area | why |
|---|---|
| the `port-raw(Phase 9)` tree, `cursor`, `MT`, `send-seam` items | Phase 9's |
| listing strategies, parse-only, downsample/denoise *ports* | sessions B/C (A only makes the silent pair refuse) |
| screen content, the D-scr-1 guard | Phase 10 |
| the POC tiebreak (`decoder_conformance_test.rs:238`) | D-poc-1 — report, do not change |
| perf (any bench/optimization) | D-gate-1 |
| the public ABI (`codec_api.h` surface, slot order, layouts, the seven exports) | frozen (D2, Phase 8) |

## Report back

1. Tally before/after (`155/199` → `n/199`), **by assertion site**, and the allowlist
   at the close (rows · owner).
2. Per step: commits, what the referee was and its red/green evidence (rc, counts),
   and anything the brief's facts got wrong (quote the number and the grep).
3. The census: `missing`/`dead`/`renamed` counts, the red-proof list, and the top of
   the `missing` list by size — this is session B/C's input.
4. F80's verdict with evidence; the LTR cause in one sentence.
5. Exit battery line, Miri line, span table (decode/encode, median/min/max, rows over
   5%), ratchet deltas (with reasons for any rebaseline).
6. Findings filed (number, one line each); decisions needed (D-poc-1 and any other).
