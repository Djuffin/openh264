# Phase 8 findings — the C-ABI boundary

Numbering continues the project-wide sequence; F75 is the last of Phase 7.

The phase opens with three findings already named and owned here: **F23** (and its
encoder twin), **F37** and **F41**. They are carried in `phase5_findings.md` /
`phase6_findings.md` where they were raised; what this file adds is what Phase 8
finds for itself.

---

## F76 — `SWelsDecoderContext::eVideoType` is write-only in the port, and the whole `DecodeFrame2` error-reporting block it belongs to has no port counterpart

*Phase 8 session A, 2026-08-21, found at step 0 by the duplicate census the moment
`src/api` entered its scope — the enum's two declarations disagreed about their
default, which is what sent someone to read the field.*

**Status: OPEN. Owner: session B** (the 18 thunks' bodies are B's, charter §3).

### How it surfaced

`VIDEO_BITSTREAM_TYPE` was declared twice — `api/codec_api.rs:324` and
`decoder/decoder_context.rs:699` — with `#[default]` on **different variants**:

| declaration | default |
|---|---|
| `api/codec_api.rs` | `VIDEO_BITSTREAM_SVC` (matches `VIDEO_BITSTREAM_DEFAULT`, `codec_app_def.h`) |
| `decoder/decoder_context.rs` | `VIDEO_BITSTREAM_AVC` |

The duplicate is unified at T8.A1 onto the api's declaration, and the unification is
**behaviour-neutral in this tree** — which is the finding, not the reassurance. It is
neutral only because the field the decoder's copy typed is never read.

### The field

C++ `WelsInitDecoder` (`decoder.cpp:666–671`) sets it from the caller's parameter:

```cpp
if (VIDEO_BITSTREAM_SVC == pCtx->pParam->sVideoProperty.eVideoBsType ||
    VIDEO_BITSTREAM_AVC == pCtx->pParam->sVideoProperty.eVideoBsType) {
  pCtx->eVideoType = pCtx->pParam->sVideoProperty.eVideoBsType;
} else {
  pCtx->eVideoType = VIDEO_BITSTREAM_DEFAULT;
}
```

**The port has no counterpart to that assignment.** `eVideoType` is written exactly
once, in the context constructor (`decoder_context.rs`), to `VIDEO_BITSTREAM_AVC` —
and never again, by anything. Every test and bench in the tree sets
`dec_param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT` (SVC) and the
decoder does not look.

### Its one reader, and the block that reader lives in

C++ has exactly one read of `eVideoType`, at `welsDecoderExt.cpp:836`, inside
`CWelsDecoder::DecodeFrame2`'s `if (pDecContext->iErrorCode) { … }` block. **That
whole block is absent from `decoder_decode_frame2_c`**, which calls `WelsDecodeBs`
and returns the accumulator. Read against `welsDecoderExt.cpp:813–900`, the port is
missing, in one place:

1. `dsOutOfMemory` → `ResetDecoder` → return `dsOutOfMemory` (or `dsErrorFree` on a
   successful reset). **`ResetDecoder`/`ThreadResetDecoder` have no port counterpart
   at all** — grep for either name in `src/` returns nothing.
2. `dsRefListNullPtrs` → the same reset-or-report arm.
3. the key-frame-loss notification — `bParamSetsLostFlag = true` when the NAL was a
   parameter set or an IDR **or the stream is AVC**, and EC is disabled. This is
   `eVideoType`'s reader, and with the field stuck at AVC in the port the arm would
   fire on *every* error rather than only on AVC streams — if the block existed.
4. the trace-throttling pair: `bPrintFrameErrorTraceFlag` cleared on the first error,
   `iIgnoredErrorInfoPacketCount` incremented (and wrapped at `INT_MAX`) thereafter.
   Both fields exist in the port's context and are initialised by `WelsOpenDecoder` /
   cleared by `WelsEndDecoder`; **nothing increments the counter.**
5. the concealment statistics: `dsDataErrorConcealed` OR-ed into the return, and
   `uiDecodedFrameCount` / `uiAvgEcRatio` / `uiAvgEcPropRatio` / `uiEcFrameNum`
   accumulated from `iMbEcedNum`, `iMbEcedPropNum` and `iMbNum`. The port sets
   `dsDataErrorConcealed` from three sites *inside* the decoder
   (`error_concealment.rs:971`, `decoder_core.rs:4313`, `manage_dec_ref.rs:713`), so
   the return code is not wholly lost — but **the four statistics counters are never
   written on this path**, and `DECODER_OPTION_GET_STATISTICS` is a public option.

### Why the byte gates are silent about it

Nothing here changes decoded samples: conformance 60/60 and the 2707-stream corpus
are unmoved by its absence, because every item above is a *status code, a recovery
action, or a statistic*. That is precisely the class the byte referee cannot see, and
it is why this is filed rather than fixed in passing.

### What it costs to fix

Items 3–5 are transliteration into `decoder_decode_frame2_c` plus the `eVideoType`
assignment in the port's `WelsInitDecoder` equivalent. Items 1–2 need `ResetDecoder`,
which is a new function on the api side (`WelsEndDecoder` + `WelsInitStaticMemory`
against the same context) and carries a behaviour change on OOM and null-ref-list
streams — so it wants its own covering test and its own byte gate, not a passing fix
inside another session's family.

### Two more of the same shape, found at T8.A5 while giving `DecoderConfigParam` its name

`DecoderConfigParam` exists in the port from T8.A5 (`decoder_core.rs`), doing the
copy and `InitErrorCon`. **Three statements of `decoder.cpp:649`'s version are not
in it**, and they are deliberately absent rather than overlooked — each is a
behaviour change on a path no byte gate drives, so each wants its own commit:

| `decoder.cpp` | statement | what its absence costs |
|---|---|---|
| `:654–661` | clamp `eEcActiveIdc` into `[ERROR_CON_DISABLE, …CROSS_IDR_FREEZE_RES_CHANGE]`, warn, and use the top value | an out-of-range `eEcActiveIdc` from a caller reaches `InitErrorCon` and every concealment test unchanged. `SetOption` carries the same clamp at `welsDecoderExt.cpp:528` (`WELS_CLIP3`) and the port has neither. |
| `:663–664` | `if (pParam->bParseOnly) eEcActiveIdc = ERROR_CON_DISABLE` | parse-only decoding would run concealment. Inert today only because `DecodeParser` is a stub. |
| `:667–671` | `pCtx->eVideoType = …` | the finding above. |

### And a third, in the same function's caller

**A second `Initialize` on a live decoder does not rebuild the context in this port;
in the reference it does.** `CWelsDecoder::InitDecoder` calls `InitDecoderCtx` for
every context, and `InitDecoderCtx` opens with `UninitDecoderCtx (pCtx)` and then
`WelsMallocz`es a fresh one (`welsDecoderExt.cpp:407–409`). `decoder_init_c` guards
the whole construction with `if (*dec_impl).pCtx.is_null()`, so a second call
re-copies the parameters into the *existing* context and returns.

The transition is reachable through the public API and nothing in the tree drives it.
`test_decoder_reinit_does_not_inherit_reordering_slots` (T8.A4) drives
Initialize → Uninitialize → Initialize, where the port's guard does the right thing
because `Uninitialize` nulls `pCtx`; **two `Initialize`s in a row** is the case that
diverges, and after T8.A6/A7 it is also the case that keeps the previous session's
statistics, last-picture record and decode timestamps where the reference `memset`s
all three. Owner: **session B**, with the thunk bodies.
