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

**Status: CLOSED at Phase 8 session B, 2026-08-21** — `6d5645cf` (T8.B1, the two
parameter statements and the two clamps), `8007dfc7` (T8.B2, the live re-init
rebuild), `dfafc008` (T8.B3, the error block whole). Six covering tests in
`tests/decoder_error_reporting_parity.rs`, each measured red against the tree that
precedes its fix, plus the throttle's observable in
`tests/trace_callback_test.rs` at T8.B6. Corpus **2707/0 codes** and conformance
**60/60** unmoved at every one of the three commits, which is the referee's answer
that none of it moved a gated code.

**Two arms are ported but uncovered, and that is measured rather than assumed**: the
`dsOutOfMemory` and `dsRefListNullPtrs` reset arms. `0x4000` and `0x40` appear in
**zero** of the 2707 corpus golden rows, and a sweep of all 62 `res/*.264` streams x
4 concealment modes x both bitstream declarations returns a state union of `0x36` —
neither bit. They are unreachable from every stream this project owns; the transcription
is by inspection against `welsDecoderExt.cpp:820-831` and says so at the site.

**One statement of the finding could not be written where the reference writes it.**
The `eEcActiveIdc` range clamp is `decoder.cpp:654`, *after* the memcpy, because in C
the field is an `int` holding whatever the caller put there. In this port it is an
eight-variant enum, so `ctx_box.pParam = *pParam` — the read the boundary did — is
undefined for exactly the inputs the clamp exists to handle: it assumed the property
the clamp was there to establish. The clamp is at the boundary (`decoder_init_c`),
on the bytes, before the block becomes an `SDecodingParam`. Same for
`sVideoProperty.eVideoBsType`, whose only reader is the `eVideoType` assignment.

*(Original brief below.)*

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

---

## F77 — a decoder panic on `res/Error_I_P.264`: a bitstream-derived macroblock index one past the end

*Phase 8 session B, 2026-08-21, found at T8.B3 by the reset-arm reachability sweep —
a probe that decoded all 62 `res/*.264` streams under four concealment modes looking
for `dsOutOfMemory`/`dsRefListNullPtrs`, and aborted on the ninth stream.*

**Status: OPEN. Owner: the decoder (Phase 9), not this session — see below.**

```text
thread panicked at src/safe/mb_grid.rs:277:23:
index out of bounds: the len is 396 but the index is 396
   3: openh264_rs::decoder::decode_slice::WelsActualDecodeMbCavlcISlice
   4: openh264_rs::decoder::decode_slice::WelsDecodeMbCavlcISlice
   5: openh264_rs::decoder::decode_slice::WelsDecodeSlice
   6: openh264_rs::decoder::decoder_core::DecodeCurrentAccessUnit
   7: openh264_rs::decoder::decoder_core::WelsDecodeBs
   8: openh264_rs::api::codec_api::decoder_decode_frame2_c
thread caused non-unwinding panic. aborting.
```

**396 is the macroblock count**, so the index is exactly one past the end — the
classic `iMbXy == kiTotalNumMbInCurLayer` off-by-one on a damaged CAVLC I-slice.

### Why it matters more than an ordinary panic

The entry point is an `extern "C"` thunk, so the unwind is a
`panic in a function that cannot unwind` and **the process aborts**. A C consumer
handing this library a damaged stream gets `SIGABRT`, not `dsBitstreamError`. That is
plan §P13's line — *"bitstream-derived values must reach error codes, never panics"* —
and it is the exact class the absent fuzzer (§0's "absent instrument" row) would have
found first. The tally in that row gains an entry.

### Provenance: pre-existing, and measured to be

The session's first instinct was that its own `DecodeFrame2` edits had caused it. They
had not: the same probe run against **`51a0956a`**, the session's start commit, with
`src/` checked out at that revision and the crate rebuilt, panics at the same line
with the same message. Nothing in T8.B1–B3 touches CAVLC macroblock decoding.

### Why it is not fixed here

`Error_I_P.264` is in `res/` but in **no** gate: not in the conformance 60, not among
the malformed corpus's eleven base streams, not in the sweeps. Fixing it means
changing a decode-path bound on damaged input — a behaviour change on an ungated
path, which is precisely the class this session's rule 4 forbids taking in passing.
It wants its own commit, its own covering test (the asset is the test), and a corpus
row so the code it *should* return is pinned. The one thing that would be wrong is to
leave it unwritten.

---

## F78 — the encoder had a second C-ABI surface, and it was dead

*Phase 8 session B, 2026-08-21, found at T8.B4 while taking step 2's thunk
inventory.*

**Status: CLOSED in the session that found it** — `4de80895`.

`wels_encoder_ext.rs` carried `G_ISVCENCODER_VTBL`, nine `ext_*` thunks duplicating
`codec_api.rs`'s nine `encoder_*_c` bodies, and
`WelsCreateSVCEncoderExt`/`WelsDestroySVCEncoderExt` — a whole parallel boundary over
`CWelsH264SVCEncoder` itself, reached by casting `*mut ISVCEncoder` straight to
`*mut CWelsH264SVCEncoder` because the struct opened with a `vptr` slot.

**Nothing called any of it.** The two factories have no caller in the crate, the
tests, the benches or the diffharness; they are not among the seven names
`codec_api.h` declares (there is no `WelsCreateSVCEncoderExt` upstream); and `vptr`
was written once by the constructor and never read. The live boundary object is
`CWelsH264SVCEncoderImpl { base, pVtbl, inner }`.

So the encoder had **two vtables, two factories and two sets of nine thunk bodies
under one interface type**, with one of each reachable. Deleted rather than carried
into step 2, where writing a `# Safety` window for a slot nothing can call would have
documented a fiction. `wels_encoder_ext.rs` raw_ptr −30, unsafe_fn −11.

**The method note.** The charter's thunk inventory counted `unsafe extern "C" fn
*_c` in `src/api`, and this set is neither `*_c` nor in `src/api`. Rule 6 —
*anchors, not surfaces; re-grep first* — is what turned it up: the count was taken
again over what the *interface type* reaches rather than over the pattern the
previous count had used.

---

## F79 — the trace callback never fired, on either codec, and the encoder had no call sites at all

*Phase 8 session B, 2026-08-21, found at step 1 where the brief told the session to
verify whether any encoder path reaches the user's trace callback. None did — and the
grep that answered it also found that none could.*

**Status: CLOSED at T8.B6** — `3fdf563f`. Three covering tests in
`tests/trace_callback_test.rs`, all three measured red against T8.B5's tree.

### What was wrong, in two independent halves

1. **`WelsLog` was a stub, twice.** `encoder/wels_encoder_ext.rs:293` and
   `decoder/decoder_core.rs:630`, each of them `let _ = (pLogCtx, iLevel, msg);`.
   Nothing in the crate read `welsCodecTrace::m_fpTrace` or `SLogContext::pfLog` —
   `grep -rn 'm_fpTrace\|pfLog' src/` returned declarations, defaults and setters,
   and no call.
2. **The encoder had zero `WelsLog` call sites.** Not a stub reached by dead
   argument, but a function with no callers on that side: `welsEncoderExt.cpp` has
   **87**. The decoder had seventeen, and they were dropped on the floor for reason
   1 plus a third: `WelsDecoderDefaults`' log-context parameter was
   `_pLogCtx: *mut c_void` and ignored, and `CWelsDecoderImpl` had no trace object to
   pass — `CWelsDecoder::m_pWelsTrace` has no counterpart in this port at all.

So `ENCODER_OPTION_TRACE_CALLBACK` and `DECODER_OPTION_TRACE_CALLBACK` — documented
options on a documented interface — were accepted, stored, and never used. The
decoder's three trace options were not even wired.

### The fix, and the one structural departure it forced

`common/wels_trace.rs` is one declaration of `SLogContext`, `WelsLog`,
`welsCodecTrace` and the six levels, retiring the census's `type SLogContext x2` and
`alias WelsTraceCallback x2`. `welsCodecTrace::m_pCodecInstance` is deleted:
`welsCodecTrace.h` has no such member (the reference's `SetCodecInstance` writes
`m_sLogCtx.pCodecInstance`) and nothing read the port's.

**The back-pointer could not be transliterated.** In C++ `SLogContext::pfLog` is
`StaticCodecTrace` and `pLogCtx` is *the `welsCodecTrace` object*, which is what lets
a `SetOption` after `Initialize` reach the copy of `SLogContext` inside the codec
context. Writing that here would store a pointer into a `Box` field of a struct every
entry point reaches through `&mut` — F38's class, and a `&mut` retag of the owner
invalidates it. It would be taken on the *logging* path, where nothing would ever
observe it going wrong. So `SLogContext` carries the settings instead of a route to
them (callback, caller's context, instance *address* as a `usize`, level), and the
six option arms re-stamp the context's copy. `SLogContext` 24 → 32 bytes; the encoder
context's offsets are re-measured, as at T6.C1/T6.D5/T7.B4/T7.C6.

### A stated divergence, not an oversight

**The reference's default sink is `welsStderrTrace`** (`welsCodecTrace.cpp:55`), with
`WELS_LOG_DEFAULT = WELS_LOG_WARNING`, so upstream writes every warning and error to
the process's stderr. **This port defaults to `None`.** Turning it on is a
library-behaviour decision with a measurable cost on this project's own instruments —
the malformed corpus alone would emit a trace line per damaged access unit across
2707 rows — and what the enumerated parity fix owed was the *installed-callback*
path, which is the one `codec_api.h` documents and a consumer can observe. A consumer
who wants the upstream behaviour installs a callback that writes to stderr.

### What is left, with an owner

The encoder now logs at `Initialize`, `InitializeExt` (the version line and the
`invalid argv` error) and `Uninitialize` — **four of the reference's 87 call sites**.
The rest, including `TraceParamInfo`'s parameter dump and `LogStatistics`, are still
stubs. They are message text on a path no gate reads, and porting 83 format strings
is not this session's work. **Owner: Phase 9**, with the rest of the encoder's
`port-raw` tree.
