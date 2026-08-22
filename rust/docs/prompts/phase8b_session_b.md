# Phase 8b — session B: parse-only, the listing strategies, F88, and the seed

*Written 2026-08-22 by the steward at `aeb011db` (session A's close). Execute top to
bottom; drop from the end. Every number below was re-grepped at that commit — re-grep
before quoting (S24), and if the tree disagrees with this brief, the tree wins and the
disagreement goes in the report.*

## Context

Phase 8b makes the port *work* before Phase 9 makes the rest of it safe (D-prio-1). The
instrument is upstream's own `test/api` suite against the Rust cdylib
(`rust/tools/abi_harness/gtest_stretch.sh --check`, gate 6c of `gates.sh exit`): **165/199
at `aeb011db`, 34 rows allowlisted, every row owned** (`tools/abi_harness/gtest_known_failures.txt`).
Session A fixed the plumbing (decoder option arms, `ForceCodingIDR`), made the tally a
ratchet, built the port-completeness census (`rust/tools/port_census.py --classify`;
`rust/docs/phase8b_port_census.md`) and made the silent pair refuse (S48). This session
takes the two feature families the census and the allowlist hand to **8b.B**, plus F88 and
the gate's seed.

## Hard rules

1. **D-gate-3.** Per commit `gates.sh commit` (+ the commit's own in-tree test). At the
   session close the fast set only (step 6). No sweeps, no Miri, no benches, no span.
2. **The reference's answer is the test.** Every feature lands with a referee that reads
   the C++'s output for the same input — `ecref`-style goldens checked in, an in-tree
   test that fails without the port and passes with it (measured red first), and the
   gtest rows moving in the same commit as the allowlist edit.
3. **S48.** Anything this session cannot finish stays a loud refusal with a test pinning
   the error code and an allowlist row naming the owner.
4. **S47.** `DecodeParser` is the last decoder slot with no referee; after this session
   it has one (in-tree and through the dlopen harness), or the report says why not.
5. **D-exit-1.** New code is safe Rust where its neighbours allow; a raw signature forced
   by a raw caller carries `// unsafe-cat: port-raw(Phase 9)` + `#[allow(unsafe_code)]`.
   `unsafe_ratchet.sh check` stays green per commit; a deliberate increase is
   rebaselined in the same commit with the reason in the message.
6. **Do not touch** `CompareOutput/39` (D-poc-1 is decided: keep; the row is permanent), the downsample/denoise family (C's),
   screen content (Phase 10), any `port-raw` conversion (Phase 9), perf.
7. Commit rhythm `T8b.B<n>`; breadcrumbs to `safety_refactor_log.md` as you go (S31);
   findings to `phase8b_findings.md` numbered from **F89**.

## Current state — verified facts at `aeb011db`

**Parse-only (`DecodeParser`) — 3 gtest rows, 2 reachable now.**
- The thunk `decoder_decode_parser_c` (`src/api/codec_api.rs:3798`) is a stub: it returns
  `dsErrorFree` and reads nothing. The reference is `CWelsDecoder::DecodeParser`,
  `codec/decoder/plus/src/welsDecoderExt.cpp:1180–1262`: bParseOnly check (else
  `dsInvalidArgument` into `iErrorCode`), `CheckBsBuffer`/`ResetDecoder`, the null-input
  arm (`bEndOfStreamFlag = true; bInstantDecFlag = true`), `iErrorCode` reset,
  `eEcActiveIdc = ERROR_CON_DISABLE`, `iFeedbackNalRefIdc = -1`, the
  `!bFramePending` reset of the internal `iNalNum` and lengths, the caller's
  `pDstInfo` zeroing and `uiInBsTimeStamp` pickup, `WelsDecodeBs(…, pDstInfo)`, the
  `dsOutOfMemory` arm, and — when `!bFramePending && iNalNum` — the whole-struct copy
  to the caller plus `uiDecodedFrameCount++`. The port's core entry is
  `Decoder::decode(src: Option<&[u8]>, ppDst, pDstInfo)` (`codec_api.rs` ~`:2453`) over
  `decoder_core::WelsDecodeBs(pCtx, kpBsBuf, kiBsLen, ppDst, pDstInfo, _pDstBsInfo: *mut c_void)`
  — the sixth parameter is **unused** in the port; upstream's `WelsDecodeBs`
  (`decoder.cpp:742`) takes `SParserBsInfo* pDstBsInfo` there.
- The machinery below the API is mostly present and **three pieces are missing**:
  1. `DecodeFrameConstruction`'s parse-only arm (`src/decoder/decoder_core.rs:1194–1275`)
     keeps the length bookkeeping but **not the byte copies** — the IDR SPS/PPS prepend
     block and the per-NAL `memcpy` into `pDstBuff`, with the two capacity checks and
     `ExpandBsLenBuffer` (`:1909`), are `decoder_core.cpp:88–175` and were deleted dead
     at T3.3 because `pNalPos` no longer existed. A NAL's payload is now the
     `sSliceBitsRead.start` offset into `sRawData` plus `iNalLength` (`nalu.rs:277–290`);
     copy from the owned buffer.
  2. The parse-only SPS/PPS/subset-SPS **bitstream copies are never written**:
     `sSpsBsInfo`/`sSubsetSpsBsInfo`/`sPpsBsInfo` exist on the context
     (`decoder_context.rs:1839–1841`, structs `:198–230`) and
     `grep -rn 'uiSpsBsLen\|pSpsBsBuf\|uiPpsBsLen\|pPpsBsBuf' src/decoder` hits only
     the struct definitions. The writers are `au_parser.cpp:1168–1200` (SPS, under
     `bParseOnly`, `SPS_PPS_BS_SIZE - 4` guard, trailing-zero trim, start-code
     prefixing), `:1240–1251` (subset SPS via `RBSP2EBSP`) and `:1480–1492` (PPS); the
     port's `ParseSps` is `nalu.rs:1545`, `ParsePps` `:1869`.
  3. `ParseOnlyBsBuffers` (`decoder_context.rs:646`) owns `pDstBuff: Vec<u8>`
     (`MAX_ACCESS_UNIT_CAPACITY`, allocation-only since T3.3) and
     `pNalLenInByte: Vec<i32>`; the boundary `SParserBsInfo` (`codec_api.rs:977`, pinned
     48 bytes) carries raw `pNalLenInByte`/`pDstBuff` pointers — the copy-out hands the
     caller pointers into those `Vec`s, valid until the next call, exactly the
     `DecodeFrame2` plane contract.
- The three rows: `DecodeParseAPI.ParseOnly_SpecSliceLoss` (`decode_api_test.cpp:953`,
  **1 spatial layer, 2 slices**) and `ParseOnly_SpecStatistics` (`:1011`, 1 layer) are
  reachable now; `ParseOnly_General` (`:873`) encodes **3 spatial layers** and fails at
  `InitializeExt` under S48 until session C's downsample lands — its allowlist row
  already says `8b.C then 8b.B`. `ParseOnly_General` decodes the parsed output with a
  second decoder and hashes it: byte-exact composition is the bar.
- **Census correction:** `port_census.py` lists `CWelsDecoder::ParseAccessUnit`
  (`welsDecoderExt.cpp:1301`) under 8b.B. Its only caller is `:1384`, inside
  `ThreadDecodeFrameInternal` — the threaded decoder, dead under D3. Reclassify it
  `dead` (the tool's classification rule, not a hand edit) and say so in
  `phase8b_port_census.md`; parse-only's real work is the list above.

**The parameter-set listing strategies — 6 gtest rows (+1 that is C's).**
- `CreateParametersetStrategy` (`src/encoder/paraset_strategy.rs:553`) returns `None` for
  `SPS_LISTING`, `SPS_LISTING_AND_PPS_INCREASING`, `SPS_PPS_LISTING`; the test at `:755`
  pins that refusal (replace it with positive tests). The port's object is one struct
  `CWelsParametersetIdStrategyObj` with `ParasetIdKind { Constant, Increasing }`
  (`:71`, `:97`) where upstream has a class tree (`codec/encoder/core/inc/paraset_strategy.h:41–280`):
  `CWelsParametersetSpsListing : IdNonConstant` (`paraset_strategy.cpp:404–537` — ctor,
  `GetNeededSubsetSpsNum`, `LoadPreviousSps`, `LoadPrevious`, `CheckParamCompatibility`,
  `CheckPpsGenerating`, `SpsReset`, `GenerateNewSps`, `UpdateParaSetNum`,
  `OutputCurrentStructure`) and `CWelsParametersetSpsPpsListing : SpsListing`
  (`:538–713` — ctor, `LoadPreviousPps`, `UpdatePpsList`, `CheckPpsGenerating`, `SpsReset`,
  `InitPps`, `UpdateParaSetNum`, `GetCurrentPpsId`, `LoadPreviousStructure`,
  `OutputCurrentStructure`, plus the free `FindExistingPps` `:608`).
  `SPS_LISTING_AND_PPS_INCREASING` is `SpsListing` with the PPS-id half of `Increasing`
  — read `CreateParametersetStrategy` in `paraset_strategy.cpp` for the exact
  construction. Two kinds more on the enum, with the overrides as kind-matched arms,
  is the port's shape; do not reintroduce a vtable.
- Hooks that are **empty today** because both ported kinds are no-ops there:
  `UpdatePpsList` (`:313`), `UpdateParaSetNum` (`:395`); and `GetCurrentPpsId` (`:399`)
  returns the identity. Upstream calls 17 distinct strategy hooks
  (`grep -ho 'pParametersetStrategy->[A-Za-z]*' codec/encoder/plus/src/welsEncoderExt.cpp codec/encoder/core/src/*.cpp | sort -u`);
  the port's grep by the same name finds two. **Map every upstream hook call site to
  its port site by reading** — a hook that is never invoked is invisible while its body
  is empty, and that is the F76/F77 signature. `LoadPrevious` is called from
  `encoder_ext.cpp:1161` (`InitDqLayers`) with `pExistingParasetList`; the port threads
  `SExistingParasetList` (`param_svc.rs:1154`) through `RequestMemorySvc`/`InitDqLayers`
  (`encoder_ext.rs:696`, `:1004`, `:1433`).
- The bitstream writer: `WriteSavcParaset_Listing` (`encoder_ext.cpp:3251–3340`: all
  `iSpsNum` SPS via `WelsWriteOneSPS`, `UpdatePpsList`, all `iPpsNum` PPS via
  `WelsWriteOnePPS`, per spatial layer) is absent; its branch in the port is the
  `ENC_RETURN_UNSUPPORTED_PARA` arm at `encoder_ext.rs:2669–2676` (`:3424–3432`
  upstream). `WelsWriteOneSPS`/`WelsWriteOnePPS` exist (`wels_encoder_ext.rs:389`, `:426`);
  `iSpsNum`/`iSubsetSpsNum`/`iPpsNum` exist on the context (`encoder_context.rs:1148–1150`).
- `ParamValidationExt`'s three strategy adjustments (`encoder_ext.cpp:466–491`: listing
  with >1 spatial layer → `CONSTANT_ID`; with `SCREEN_CONTENT_REAL_TIME` → `CONSTANT_ID`;
  with `bSimulcastAVC` → `INCREASING_ID`, each with a WARNING trace) — check the port's
  validation (`wels_encoder_ext.rs:~1294–1330`, `:863` is the param-adjust side) and port
  what is missing. `param_svc.rs:688–694` already accepts all five enum values.
- The rows: `EncodeDecodeTestAPI.ParameterSetStrategy_{SPS_LISTING_AND_PPS_INCREASING1,2,3,
  SPS_PPS_LISTING1,2,3}` (`encode_options_test.cpp:556/656/705/772/844/911`), all failing at
  `InitializeExt` (rv 1). Each re-initialises mid-stream with changed parameters and
  checks the SPS/PPS ids and decodability — read the six bodies before designing the
  referee. `SimulcastAVC_SPS_PPS_LISTING` needs multi-layer and stays C's.

**F88 and the seed.** `test/api`'s `main` (`test/api/simple_test.cpp:20–24`) seeds
`rand()` from `time(NULL)` unless its first non-gtest argument is `--seed=N`, and prints
`Random seed: N`. `gtest_stretch.sh`'s `run_one` passes only `--gtest_filter`; every
`--check` run is therefore a different stream set, and
`EncodeDecodeTestAPI.SetOptionECIDC_SpecificFrameChange` was red **once in seven** runs
at `5478ae7e` (`decoder_ec_test.cpp:302`, `EXPECT_TRUE(rv != 0)` — the port's concealment
returns success where the reference returns an error on that seed). Not allowlisted,
correctly (F88). A filter changes the `rand()` sequence, so the seed pins the *suite*, not
the test.

**Stale "unported" prose.** `encoder_ext.rs:3022` ("Unported branches": MT and
`SM_SIZELIMITED_SLICE` — both ported in Phase 7) and `:993–997` (AQ/background
buffers — `processing/` has both). Leave code alone; fix the sentences you can prove
wrong by grep.

## Steps

### Step 0 — open, and pin the seed
`gates.sh commit`. Then `gtest_stretch.sh`: pass a fixed `--seed=` in `--check` (one
constant, documented at the top of the file; the same seed for both links) so the gate
is deterministic; add `--seeds=A..B` (full suite per seed, the per-seed failing rows
printed, ~40 s each) as the finder. Red-proof: `--check` twice → identical tallies;
`--seeds` over three seeds prints three `Random seed:` lines.
**Accept:** `--check` rc 0 at 165/199 with the pinned seed; the two modes documented.

### Step 1 — parse-only
Port the three missing pieces and the thunk, statement for statement against
`welsDecoderExt.cpp:1180–1262`, `decoder_core.cpp:88–175`, `au_parser.cpp:1168–1200/
1240–1251/1480–1492`. Referee first: `ecref --parse-only` (reference lib, `DecodeParser`
per AU + the trailing `(NULL, 0)` call, printing per call `rv iNalNum [lengths] w h
inTs outTs sha1(composed bytes)`), goldens under `tests/data/decoder_parseonly/` for at
least four assets (include one multi-slice and one with SPS/PPS repeats), and
`tests/decoder_parseonly_parity_test.rs` driving the C API in parse-only mode —
**measured red against the stub**, green after. Add the part to
`tools/abi_harness/abi_harness.cpp` (the dlopen side), rows extracted from the test at
run time like the nodelay part. Then the two gtest rows off the allowlist.
**Accept:** parity test green on every golden line; harness TALLY +1 part; `--check`
167/199 (or the exact number, with the by-site tally); `ParseOnly_General` row re-owned
`8b.C then 8b.B` with the reason unchanged.

### Step 2 — the listing strategies
Two new `ParasetIdKind`s with their overrides, `FindExistingPps`,
`WriteSavcParaset_Listing`, the validation adjustments, and the hook-call map (every one
of the 17 upstream hook sites named with its port line in the log). Referee: the six
gtest rows, plus byte parity with the reference for each strategy — `rust_enc`/`cxx_enc`
gain `--paraset-strategy <0..4>` and a `--reinit-at <frame>` knob that re-runs
`InitializeExt` with a changed size the way the gtest bodies do; **one targeted pair per
strategy** in the session (D-gate-3), the configurations recorded so the phase close can
add them to a `ps` sweep preset. Replace `unported_strategies_return_none` with
positive tests.
**Accept:** the six rows pass and leave the allowlist; each strategy's targeted pair
byte-identical; ratchet explained.

### Step 3 — F88
With `--seeds`, find a seed on which `SetOptionECIDC_SpecificFrameChange` fails on the
Rust link and passes on the C++ link; record it in the finding. Then find the
divergence: the seed pins the suite's streams, so reproduce with the full suite at that
seed (a test-binary dump hook under an env var is acceptable — it is test tooling, not
product), extract the stream and the loss pattern, and compare the two decoders'
per-call `rv`/`iBufferStatus` with `ecref`/`portref`. Fix if the cause is in the
concealment path and the fix is one enumerated behaviour change with a covering test;
otherwise file the cause with the seed and the stream and hand it on. Time-box 2 h.
**Accept:** F88 has a reproducing seed and a named cause; fixed or owned.

### Step 4 — census and prose
`port_census.py`: `ParseAccessUnit` → `dead` by rule; regenerate
`phase8b_port_census.md`'s tables (the 8b.B column reads what is left). Fix the two
stale comment blocks. Re-run `find_stub_bodies.py` on the functions this session added.
**Accept:** census 8b.B = 0 open rows (or the residue owned by name); no new stub.

### Step 5 — what to drop if short
Drop from the end: step 4's prose, then step 3's fix (keep the seed and the cause),
then step 2's `--reinit-at` harness knob (keep the gtest rows + one pair per strategy
by hand). Never drop step 0 or the referees of steps 1–2.

### Step 6 — close (the fast set, D-gate-3)
`gates.sh commit`; `gtest_stretch.sh --check`; `abi_exports.sh release`;
`abi_harness/run.sh`; `ecref/compare_all.sh`. The log entry; `phase8b.md` §5's B row;
report.

## Do not touch

| item | owner |
|---|---|
| `CompareOutput/39` (POC tiebreak) | D-poc-1 — decided 2026-08-22: keep; permanent row |
| downsample/denoise, the 17 init-refusal rows, `ParseOnly_General`'s layers | 8b.C |
| F80/F87 (`Increase`/`DecreasePicBuff`, `safe::Pool` growth) | 8b.C |
| screen content (7 rows, census 19) | Phase 10 |
| any `port-raw(Phase 9)` signature conversion, F84, F85, F86's open half | Phase 9 |
| perf, sweeps, Miri, benches | the phase close |

## Report back

1. Commits (hashes, one line each). 2. The tally before/after by assertion site, and the
allowlist diff. 3. Per step: what landed, the red-proof evidence (the measured-red line
for each referee), anchors. 4. F88: seed, cause, status. 5. Session-close lines (commit
gate, `gtest --check`, exports, harness TALLY, `compare_all`), ratchet deltas with
reasons. 6. Findings F89+. 7. Brief facts that did not survive, quoted. 8. What was
dropped and why; what C inherits.
