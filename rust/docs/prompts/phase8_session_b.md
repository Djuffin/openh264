# Phase 8, session B — the encoder boundary owns, the thunks translate, and F76's dropped error block

You are executing the second session of Phase 8 (charter:
`rust/docs/prompts/phase8.md`). Work the steps in order, commit per unit, run
the gates as stated, report in the format at the end.

## Context

- Repo `/Users/eugene/projects/openh264`, branch `rust3`; crate
  `rust/crates/openh264-rs/`, paths relative to its `src/`. C++ at `codec/` is
  the reference. **The public C/C++ ABI (`codec/api/wels/codec_api.h`) is the
  contract and does not change** — vtable structs, slot order, factories,
  exported names, boundary struct layouts. This phase moves the safety line
  *up to* that surface; it never moves the surface.
- Start: commit `60ca7929`, tree clean. Session A: the api inventory done
  (five instruments reach `src/api`; four duplicate boundary types fixed, two
  enumerated); **F23 closed on both codecs** (19 conveniences → associated
  functions taking `this: *mut _`, 116 callers re-spelled, covering test red
  → green under Miri); **F41 closed** (the context owns its `pParam`;
  `CWelsDecoderImpl::param` had no counterpart in the reference and is gone);
  **F37 proved** (already fixed at T5.O1; the public-API re-init probe is
  new); the twelve api-owned decoder fields owned by the context — stamps 0,
  `api_alias` 0, the decoder's production allow items 0; `memory_align.rs`
  deleted. `CWelsDecoderImpl` is now `{ base, pVtbl, pCtx: *mut
  SWelsDecoderContext, bEndOfStream }`.
- This session: the **encoder** boundary object owns what it points at, the
  **19 thunks** become translators with written contracts, the **safe cores**
  are carved, the **trace plumbing** delivers messages, the **`c_void` line**
  is attributed — and **F76**, the decoder's dropped error-reporting block,
  is ported with covering tests.
- Docs at close: `safety_refactor_log.md`, plan §0 row, `phase8.md` §3,
  `perf_baseline.md`, `phase8_findings.md`.
- Commits: `refactor(T8.B<n>)` / `fix(T8.B<n>)` / `docs(T8.B-close)`.

## Hard rules

1. **Gates**: `gates.sh commit` per commit; `family` per step; `exit` at the
   close, **unscoped** (both codecs touched). Conformance 60/60, corpus 2707
   rows agreeing on output *and* codes, sweeps 369/369 both profiles, benches
   bit-identical.
2. **ABI untouched** (above). `api/abi_guard.rs` asserts move only if a
   non-boundary struct changes.
3. **F23's rule**: a thunk's `this` derives from the whole impl allocation;
   no `&mut self` on `ISVCDecoder`/`ISVCEncoder`.
4. **Behavior changes are enumerated parity fixes only** — F76's arms and the
   trace delivery — each with a covering test that is red before and green
   after, and the byte/code gates unmoved. Anything else that moves a byte or
   a return code is a defect.
5. **Wrong-length sweep output is a defect.** Stop and fix.
6. **Anchors, not surfaces** (2026-08-21, A's close). Re-grep first; the
   unit matters (A's `c_void` count was occurrences, the charter's was lines).
7. **Perf**: one span at the close, both benches, 7 pairs + null; bisect on
   a breach before acting.
8. **Overflow**: stop green at a whole-family boundary; C inherits.

## Current state — verified facts

- **The encoder boundary object** `CWelsH264SVCEncoder`
  (`encoder/wels_encoder_ext.rs:1576`): `vptr`, `m_pEncContext: *mut
  sWelsEncCtx` (**83 mentions**, 8 `*mut sWelsEncCtx` boundary lines; created
  by `WelsInitEncoderExt(ppCtx: *mut *mut sWelsEncCtx, …)` at `:1769`, nulled
  at `:1795`), `m_pWelsTrace: *mut welsCodecTrace` (**31 mentions**;
  `Box::into_raw` at `:1594`, `Box::from_raw` at `:2484`), `m_iMaxPicWidth/
  Height`, `m_iCspInternal`, `m_bInitialFlag`. Methods `:1593–:1872+`:
  `new`, `InitEncoder`, `GetDefaultParams`, `Initialize`, `InitializeExt`,
  `InitializeInternal`, `Uninitialize`, `EncodeFrame`, `EncodeFrameInternal`,
  `EncodeParameterSets`, `ForceIntraFrame`, `SetOption`, `GetOption`. The
  object itself is `Box::into_raw`'d at `WelsCreateSVCEncoder`
  (`:2598`) and `from_raw`'d at destroy (`:2606`). 136 raw tokens, 15
  `unsafe fn`, 14 `extern "C"` in the file.
- **The trace plumbing**: `WelsTraceCallback` is declared **twice**
  (`wels_encoder_ext.rs:240`, `codec_api.rs:1074` — A's inventory enumerated
  it for you); `welsCodecTrace { m_iTraceLevel, m_fpTrace, m_pTraceCtx: *mut
  c_void, m_sLogCtx: SLogContext, m_pCodecInstance: *mut c_void }`;
  `SLogContext { pfLog: *mut c_void, pLogCtx: *mut c_void, pCodecInstance:
  *mut c_void }` (`encoder_context.rs:88` — three erased pointers; the
  decoder's `SLogContext` at `decoder_context.rs:663` already types `pfLog`
  as `Option<unsafe extern "C" fn>`); `SetCodecInstance(self as *mut Self as
  *mut c_void)` at `:1613` is the back-pointer (3 uses); the callback is
  installed through `SetOption` at `:2356`. **`WelsLog`
  (`wels_encoder_ext.rs:293`) is a stub: `let _ = (pLogCtx, iLevel, msg);`** —
  verify whether *any* encoder path reaches the user's trace callback
  (`grep -rn 'm_fpTrace\|pfLog' src/encoder` — who calls the pair); if none,
  that is a parity finding (the C++ logs through `WelsCodecTrace::CodecTrace`
  at init, param adjust, and errors) — record it, fix it under rule 4.
- **The thunks**: 19 (`unsafe extern "C" fn *_c` in `codec_api.rs`; a
  digit-free grep misses `decoder_decode_frame2_c`). Shape today:
  null-check → `this as *mut CWels…Impl` → `(*impl).inner.Method(raw args)`.
  Target: null-check → impl derivation → **translate-in** (raw args to
  references/slices with the C-caller's validity window stated) → **safe
  call** → **translate-out**, each with a `# Safety` contract comment naming
  the window.
- **`c_void`**: **58 occurrences** across `src/encoder` + `src/api` —
  `codec_api.rs` 17, `wels_encoder_ext.rs` 12, `encoder_context.rs` 10 (the
  `SLogContext` trio + context fields), `slice_multi_threading.rs` 6,
  `wels_preprocess.rs` 2, `svc_encode_slice.rs` 2, `md.rs` 2,
  `wels_func_ptr_def.rs` 1.
- **F76** (`phase8_findings.md:12`), the decoder's `DecodeFrame2` error block.
  The reference is `codec/decoder/plus/src/welsDecoderExt.cpp:815–900`; the
  port's `decoder_decode_frame2_c` is `codec_api.rs:2036`. What the port
  lacks, per the finding and the C++ read:
  - after `WelsDecodeBs`: `bInstantDecFlag = false`; then **if
    `iErrorCode != 0`**: on `dsOutOfMemory` / `dsRefListNullPtrs` →
    `ResetDecoder` and return (or `dsErrorFree` if the reset fails its own
    check); the **key-frame-loss notification** (`bParamSetsLostFlag` under
    `LONG_TERM_REF`, else `bReferenceLostAtT0Flag`) when the NAL is a
    param-set/IDR or `eVideoType == VIDEO_BITSTREAM_AVC` and EC is
    disabled; the **trace throttle** (`bPrintFrameErrorTraceFlag` /
    `iIgnoredErrorInfoPacketCount`); when EC is active and
    `pDstInfo->iBufferStatus == 1`: `iErrorCode |= dsDataErrorConcealed`, the
    four **EC statistics** updates (`uiDecodedFrameCount` with its wrap
    reset, `uiAvgEcRatio`, `uiAvgEcPropRatio`, `uiEcFrameNum`); `dDecTime`;
    `OutputStatisticsLog`; then the buffering/reordering branch and
    `return iErrorCode`.
  - three `DecoderConfigParam` statements: the EC clamp, the parse-only EC
    disable, `eVideoType`.
  - a second `Initialize` on a live decoder **rebuilds** the context in the
    reference and does not here.
  None of it moves a corpus byte or code under the corpus's configurations
  — which is why it survived; the covering tests must reach each arm.

## Step 0 — F76, test-parity first

1. For each arm, a covering test that reaches it through the public API,
   red before: a corrupt/missing-reference stream for the reset arms (if an
   arm is unreachable from any stream, say so and skip it with the reason);
   an AVC stream with EC disabled for the key-frame-loss flag; an EC-active
   concealed frame for the statistics (`GetOption(DECODER_OPTION_GET_STATISTICS)`
   before/after); a double-`Initialize` test; a parse-only EC-disable test.
2. Port the block line-for-line in the C++'s order into
   `decoder_decode_frame2_c` (check whether `DecodeFrameNoDelay`/`Ex`/`Parser`
   share the tail in the C++ and mirror that); the three
   `DecoderConfigParam` statements; the live-re-init rebuild.
3. `eVideoType` gains its reader (the finding's origin).

Accept: each test green; conformance 60/60 and corpus **2707/0 codes**
unmoved (the corpus is the referee that none of this moved a gated code);
`family` green.

## Step 1 — the encoder boundary owns

1. `m_pEncContext: Option<Box<sWelsEncCtx>>` — `WelsInitEncoderExt` returns
   the `Box` (or fills `&mut Option<Box<_>>`); `WelsUninitEncoderExt` takes it
   by value; the 8 boundary lines derive `&mut` from the `Box` once per call
   (S42: this is the allocation root, the one place `&mut sWelsEncCtx` is
   born — the tree below still takes raw and stays tagged).
2. `m_pWelsTrace: Box<welsCodecTrace>`; `:1594`/`:2484` die.
3. `m_pCodecInstance` / `SLogContext::pCodecInstance`: read their readers —
   if none (S18) delete; if the log callback needs the instance, it is the
   `pLogCtx` the user supplied, not a back-pointer.
4. `SLogContext` typed: `pfLog: WelsTraceCallback` (one declaration — delete
   the duplicate, A's inventory item), `pLogCtx: *mut c_void` (**C-ABI**: the
   user's opaque context, stays raw, tagged).
5. **`WelsLog` delivers**: format the message and call the pair exactly as
   `WelsCodecTrace::CodecTrace` does (level filter, then
   `pfLog(pLogCtx, level, cstr)`). Covering test: install a callback via
   `ENCODER_OPTION_TRACE_CALLBACK` + `_CONTEXT`, run `Initialize`, assert the
   callback fired at least once with the level the C++ uses there; red
   before if the stub is what you found. Decoder side: verify its `pfLog`
   path fires the same way; fix if not.

Accept: `grep -rn 'm_pEncContext\|m_pWelsTrace' src/encoder | grep -v '//'`
shows only owned spellings; `grep -rn 'pub type WelsTraceCallback' src/` → 1;
the trace test green; `family` green.

## Step 2 — the 19 thunks as translators

Per thunk: null checks as today → impl derivation (rule 3) → translate-in
(`*const SSourcePicture` → `&SSourcePicture` for the call's duration;
`*mut SFrameBSInfo` → `&mut`; `kpSrc, kiSrcLen` → `&[u8]`; option blobs by
`ENCODER_OPTION_*`/`DECODER_OPTION_*` discriminant as today) → the impl
object's safe method → translate-out (`ppDst` plane pointers, `pDstInfo`
fields — written from the decoder's owned planes with the validity window
the C caller assumes: until the next decode call on this decoder, which is
what `codec_api.h` documents). A `# Safety` comment on each thunk naming
the window and the null/length contract. The impl methods lose `unsafe fn`
where ownership now permits (`InitializeInternal`, `EncodeFrameInternal`,
…) — count before/after.

Accept: 19 thunks, 19 `# Safety` contracts; `unsafe fn` in
`wels_encoder_ext.rs` 15 → the enumerated remainder; `family` green; Miri's
`abi_test_driver` and all probes green.

## Step 3 — the safe cores, and the decoder impl's box

1. `CWelsDecoderImpl::pCtx: *mut SWelsDecoderContext` → `Option<Box<_>>`
   (the context is constructor-built since 5b; `decoder_init_c` already
   builds it with `new_boxed`). Thunks derive `&mut` from it.
2. Carve and export the safe Rust API: `pub struct Decoder(Box<SWelsDecoderContext>)` /
   `pub struct Encoder(…)` (or the impl inners, renamed — naming is yours,
   say why) with the methods Rust consumers need (initialize / decode /
   encode / options / statistics) as **safe** methods; `assert_send::<Decoder>()`
   and `::<Encoder>()` compile tests (a `fn assert_send<T: Send>() {}` called
   in a test). If one is not `Send`, the `E0277` list is the inventory —
   record it, do not force it.

Accept: the compile tests exist and state the verdict; the C-ABI impl
objects wrap the safe cores; all batteries green.

## Step 4 — the `c_void` line

Attribute all 58: **C-ABI** (user contexts, the callback pair's context, the
`pOption` blobs at Set/GetOption) stay raw and tagged; anything else is
typed or deleted (the `SLogContext` trio, `m_pCodecInstance`, the
`encoder_context.rs` fields — read each). Table in the log: occurrence →
verdict.

Accept: `grep -rho 'c_void' src/encoder src/api | wc -l` = the C-ABI set
only, each tagged.

## Step 5 — close

1. The span (both benches), unscoped `gates.sh exit`.
2. Log: F76's arms and tests, the trace finding (if any) and its fix, the
   ownership changes, the 19 contracts, the cores' `Send` verdicts, the
   `c_void` table; what C inherits: `crate-type`, the 7 exports (two
   `#[no_mangle]`s at `wels_encoder_ext.rs:2620`/`:2627`), `api/abi_guard.rs`
   pins for every boundary struct (`SParserBsInfo`, `OpenH264Version`,
   `SDecoderCapability`, …), the external-ABI harness (template:
   `tools/diffharness/cxx_enc.cpp`; gtest tree at `test/api`), the `api/`
   allow endgame, and `SParserBsInfo`'s two-entities rename (D5/Phase 9).
3. Plan §0 row (B spent, C next); `phase8.md` §3; `perf_baseline.md`.

## Do not touch

| what | why |
|---|---|
| vtable structs, slot order, factories, exported names, boundary struct layouts | the ABI contract |
| `crate-type`, `#[no_mangle]` additions, the external harness, the `api/` allow endgame | session C |
| the slice-encode tree's `port-raw(Phase 9)` signatures below the boundary; parked families; `SCREEN_CONTENT(dormant)` | later phases |

## Report back, in this order

1. One line: steps landed; `exit` verdict; HEAD; tree state.
2. F76: arms ported, tests (red/green each), corpus codes unmoved.
3. The trace: what `WelsLog` did, what it does now, the test.
4. Ownership: `m_pEncContext`/`m_pWelsTrace`/`pCtx` spellings; `unsafe fn`
   before → after in the two files.
5. The 19 contracts; the cores and their `Send` verdicts.
6. The `c_void` table summary (C-ABI count, deleted, typed).
7. The span.
8. Anything found and not fixed, with owner; what C inherits exactly.
