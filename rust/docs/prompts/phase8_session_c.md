# Phase 8, session C — the cdylib, the seven exports, the external harness, and the phase close

You are executing the last session of Phase 8 (charter:
`rust/docs/prompts/phase8.md`). Work the steps in order, commit per unit, run
the gates as stated, report in the format at the end.

## Context

- Repo `/Users/eugene/projects/openh264`, branch `rust3`; crate
  `rust/crates/openh264-rs/`, paths relative to its `src/`. C++ at `codec/` is
  the reference. **The public ABI (`codec/api/wels/codec_api.h`,
  `codec_app_def.h`) is the contract and does not change.**
- Start: commit `5876d802`, tree clean. After B: the impl objects own their
  contexts and traces (`Decoder`/`Encoder` newtypes carved; neither is `Send`
  — 14 `E0277`s, all in the `port-raw(Phase 9)` tree, measured by a macro that
  reports both answers); the 19 thunks carry `# Safety` contracts; the trace
  delivers (F79); F76 closed with seven covering tests; F78's dead second
  C-ABI surface deleted; the `c_void` line is 26 code occurrences, all
  `C-ABI`.
- This session makes the drop-in real: the crate ships as a `cdylib` that
  exports exactly upstream's seven symbols, every boundary type is pinned to
  the C++'s own sizes, an external C++ driver proves the in-process hashes
  reproduce through `dlopen`, no panic crosses the ABI (P13, with F77 fixed),
  `api/` gets its deny, and the phase closes.
- **Two rulings you execute** (plan §7.4): **D-api-1** — the default trace
  sink matches upstream (`welsStderrTrace` at `WELS_LOG_WARNING`) — a drop-in
  that is silent where the reference speaks is a divergence; and **F77 is
  this session's, not Phase 9's** — a decoder panic that aborts a C host
  process on one malformed stream is a boundary defect, and P13's guard is
  boundary work.
- Docs at close: `safety_refactor_log.md` (session + phase-close entries),
  plan §0 (**Phase 8 COMPLETE** row, Phase 9 next with its inheritance
  appended), `phase8.md` §3, `perf_baseline.md`, `phase8_findings.md`.
- Commits: `refactor(T8.C<n>)` / `fix(T8.C<n>)` / `gate(T8.C<n>)` /
  `docs(T8.C-close)`.

## Hard rules

1. **Gates**: `gates.sh commit` per commit; `family` per step; the close is
   the full unscoped `exit` **plus the two gates this session adds** (step 1's
   export check, step 3's harness) wired into `gates.sh exit`.
2. **ABI untouched**: names, slot order, layouts. `api/abi_guard.rs` only
   *gains* pins. Renaming the decoder-*internal* `SParserBsInfo` (step 4) is
   not an ABI change — the boundary struct keeps its name.
3. **F23's rule** stands (a thunk's `this` derives from the whole impl).
4. **Behavior changes are the enumerated ones only** — F77's index fix, P13's
   guard, D-api-1's default sink — each with a covering test red before, and
   conformance 60/60 / corpus 2707 agreeing on output *and* codes / sweeps
   369/369 unmoved. Anything else that moves a byte or a code is a defect.
5. **Wrong-length sweep output is a defect.**
6. **Anchors, not surfaces** (2026-08-21, B's close). Re-grep; state units.
7. **Perf**: one span at the close, both benches, 7 pairs + null.
8. **Overflow**: stop green; a session D closes the phase.

## Current state — verified facts

- **Exports**: 5 of 7. `WelsGetCodecVersion` / `WelsGetCodecVersionEx` at
  `encoder/wels_encoder_ext.rs:2567` / `:2574` carry `#[allow(unsafe_code)]`
  but **no `#[no_mangle]`**. No `crate-type` in `Cargo.toml` (rlib only).
- **Pins**: `api/abi_guard.rs` — 50 `assert_*` lines pinning **11** boundary
  types (`OpenH264Version SBitrateInfo SDecodingParam SEncParamBase
  SEncParamExt SEncoderStatistics SFrameBSInfo SLayerBSInfo SSliceArgument
  SSourcePicture SSpatialLayerConfig`). B named **seven** genuine boundary
  structs unpinned: `SBufferInfo`, `SDecoderCapability`, `SDecoderStatistics`,
  `SParserBsInfo`, `SSysMEMBuffer`, `SVideoProperty`, `SVuiSarInfo`. B
  counted 27 boundary types in all — derive the full list from the two
  headers (every `typedef struct`/`union`/`enum` a C caller can pass or
  receive), not from the port.
- **`api/codec_api.rs`**: 2 `#[allow(unsafe_code)]` items (both on
  `SBufferInfo`'s ABI-union accessors); **no `deny` yet**. The 19 thunks are
  `unsafe extern "C" fn`, the 19 `*_raw` conveniences likewise — under a
  module `deny` each becomes a tagged allow item, category `C-ABI` by
  construction.
- **F77** (`phase8_findings.md:154`): `res/Error_I_P.264` → panic at
  `safe/mb_grid.rs:277` (index 396 of 396, a damaged CAVLC I-slice) inside
  `WelsActualDecodeMbCavlcISlice`, crossing `decoder_decode_frame2_c` →
  `thread caused non-unwinding panic. aborting.` Found by a probe decoding all
  62 `res/*.264` under four concealment modes; the asset is in **no** gate
  today (`tools/ecref/compare_all.sh`, `tests/malformed_stream_parity.rs`,
  `tests/decoder_conformance_test.rs` — none names it). `catch_unwind` appears
  **0** times in `src/api`. Plan **P13** (§2, line ≈407): bitstream-derived
  values must reach error codes, never panics; the C++ error paths exist.
- **The trace default**: `common/wels_trace.rs` (B) delivers; the default sink
  is `None`; upstream's `welsCodecTrace()` constructor installs
  `welsStderrTrace` at `WELS_LOG_WARNING`.
- **Harness assets**: `tools/diffharness/cxx_enc.cpp` is a C++ driver built
  against `codec/api/wels` (see `tools/diffharness/build.sh:10` for the
  compile line); the in-process decoder goldens live with
  `tests/decoder_conformance_test.rs`; `rust_enc` (the sweep's encoder driver,
  a separate cargo project under `tools/diffharness/rust_enc/`) produces the
  in-process encode bytes. Upstream's gtest API suite: `test/api/*.cpp` (15
  files), built by the root `Makefile` with `build/gtest-targets.mk`.

## Step 0 — P13: no panic crosses the ABI, and F77 is fixed

1. **The index fix**: read the C++'s handling of the same damaged slice
   (`decode_slice.cpp`'s CAVLC I-slice MB loop — the bound that turns
   `iMbXy >= kiTotalNumMb` into an error return) and port it; the decode
   returns `dsBitstreamError` (or whatever code the C++ returns for this
   asset — run the reference on it and match). Add `res/Error_I_P.264` to the
   corpus rows (`compare_all.sh` and `malformed_stream_parity.rs`) so its
   output and codes are gated against the C++ from now on.
2. **The guard**: every thunk body runs inside `std::panic::catch_unwind`
   (`AssertUnwindSafe`); a caught panic maps to the entry's failure code
   (decode entries → `dsBitstreamError`-class, `Initialize` → the init error,
   encode entries → `cmUnknownReason`, `GetOption`/`SetOption` → the option
   error) and logs through the trace at `WELS_LOG_ERROR`. **Read the crate's
   `panic` profile first**: if any shipped profile sets `panic = "abort"`, the
   guard cannot catch there — say so in the log and make P13 rest on line 1
   (error codes at the source) for that profile. Covering test: a `cfg(test)`
   panic hook inside one decoder and one encoder entry → the mapped code, the
   process alive.
3. The probe that found F77 (62 assets × 4 EC modes) becomes a standing test
   or a corpus extension — it is the reachability sweep the reset arms never
   had; keep it cheap (one pass, release) and say where it lives.

Accept: `Error_I_P.264` in the corpus with codes agreeing; the guard test
green in both profiles where unwinding exists; conformance/corpus/sweeps
unmoved; `family` green.

## Step 1 — the cdylib and the seven exports

1. `crate-type = ["rlib", "cdylib", "staticlib"]`.
2. `#[no_mangle]` on the two version functions; their `#[allow]` tags become
   `C-ABI`.
3. `tools/abi_exports.sh`: builds the cdylib, runs `nm -g` (platform-aware),
   and diffs the exported-symbol list against exactly
   `WelsCreateSVCEncoder WelsDestroySVCEncoder WelsCreateDecoder
   WelsDestroyDecoder WelsGetDecoderCapability WelsGetCodecVersion
   WelsGetCodecVersionEx` — **no more, no fewer** (a Rust cdylib exports only
   `#[no_mangle] pub extern` items, but check for accidental ones); wired into
   `gates.sh exit` as a step. Red-proof: drop one attribute locally, watch it
   fail, restore.

Accept: the new gate green with 7/7; the red-proof recorded.

## Step 2 — every boundary type pinned to the C++'s numbers

1. `tools/abi_sizes.cpp`: a tiny C++ program compiled against the upstream
   headers (the `cxx_enc` compile line) that prints `sizeof`/`alignof` (and
   `offsetof` for the fields the port reads by position) for every boundary
   type in the derived 27-list; commit it and its output.
2. `api/abi_guard.rs` pins all 27 from that output — the seven named structs
   first — with the union `SBufferInfo` and every enum's size included; a pin
   that disagrees with the port's layout is a **finding**, not a number to
   adjust (read which side is wrong against the header).

Accept: 27/27 pinned; `cargo test` green; the dumper committed.

## Step 3 — the external-ABI harness

`tools/abi_harness/`: a C++ driver compiled against `codec/api/wels` that
`dlopen`s the cdylib (or links the staticlib — do the dlopen form; it is what
a drop-in consumer does), resolves the seven symbols with `dlsym`, and runs:

1. **Decoder conformance**: the 60 assets through `WelsCreateDecoder` →
   `Initialize` → `DecodeFrameNoDelay`/`DecodeFrame2` as the in-process test
   does → per-asset hashes **equal the in-process goldens**.
2. **Encode loopback**: a dozen diffharness configurations (spanning the
   presets) through `WelsCreateSVCEncoder` → bytes **equal `rust_enc`'s
   in-process bytes**, both profiles.
3. The version functions return upstream's values; `WelsGetDecoderCapability`
   matches.
4. The F77 asset returns an error code through the dylib with the process
   alive.
Wired into `gates.sh exit` as a step with its own tally line.

**Stretch (time-boxed, one hour)**: point upstream's `test/api` gtest build at
the Rust cdylib and report pass/fail counts beside the reference's — a number,
not a promise.

Accept: harness green on all four parts; `gates.sh exit` runs it; the
stretch reported either way.

## Step 4 — D-api-1, the `api/` deny, and the rename

1. **D-api-1**: the default trace sink is the stderr writer at
   `WELS_LOG_WARNING`, exactly upstream's; tests that need silence install a
   quiet callback explicitly (as a C consumer would). Covering test: a fresh
   decoder and encoder with no callback set log a WARNING-level message to
   stderr in a case the C++ does (capture stderr in the test).
2. `#![deny(unsafe_code)]` on `api/codec_api.rs` and `api/abi_guard.rs`; every
   surviving item allowed + tagged (`C-ABI` for thunks, exports, `*_raw`
   conveniences, the union accessors; anything else named with its reason);
   the count recorded. This supersedes the plan's "api/ gets a module-wide
   allow" with D-exit-1's regime — every item owned.
3. Rename the decoder-internal `SParserBsInfo` (`decoder/decoder_context.rs:635`)
   so the name means one thing (the boundary struct at `codec_api.rs:958`
   keeps it); D5's Phase-9 naming pass inherits only the Wels-style question.

Accept: `grep -rln 'deny(unsafe_code)' src/api` → 2; allows = tags; the
stderr test green; one `SParserBsInfo` in the tree.

## Step 5 — the phase closes

1. The span (both benches), then `gates.sh exit` unscoped with the two new
   gates in it.
2. Phase-close log entry: Phase 8's arc (api 2,788 lines at open → final
   shape; F23/F37/F41/F76/F78/F79 closed, F77 fixed here; the inventory; the
   7 exports; the 27 pins; the harness numbers; the gtest stretch's number).
3. Plan §0: **Phase 8 COMPLETE** row (model: Phase 7's), sessions A–C spent,
   **Phase 9 next** — and append to §4 Phase 9's inheritance block, verified
   against the tree: the 705 `port-raw(Phase 9)` items and the 22 dispatch
   survivors; **F73** (the picture-accessor `&mut` family, 32 + 68 sites);
   **F66/S42** (the context-parameter split, 93 caller sites, the detector);
   the `Send` verdicts (14 `E0277`s, 3 decoder / 11 encoder); the crate-root
   `deny` flip with `api/` already denied; D4 (workspace split) and D5
   (naming) decisions; the parked perf families and D-perf-6 with cumulative
   ≈+15…17% restated.

## Do not touch

| what | why |
|---|---|
| ABI names, slot order, layouts; boundary struct names | the contract — pins only add |
| the `port-raw(Phase 9)` tree, `SliceJobHandle`, the 22 dispatch survivors | Phase 9 |
| parked perf families; `SCREEN_CONTENT(dormant)` | Phases 9 / 10 |

## Report back, in this order

1. One line: phase closed or not; `exit` verdict incl. the two new gates;
   HEAD; tree state.
2. P13/F77: the index fix and its code, the guard's profile verdict, the
   corpus row added.
3. Exports: 7/7, the red-proof.
4. Pins: 27/27, any disagreement found.
5. The harness: the four parts' results; the gtest stretch number.
6. D-api-1, the `api/` deny counts, the rename.
7. The span.
8. Anything found and not fixed, with owner; Phase 9's inheritance as
   appended.
