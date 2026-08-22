# Phase 8b findings

*Numbering continues Phase 8's (which closed at F83). Each entry is a fact with the
grep or the run that produced it.*

## F84 — two threaded-decoder functions survive in the port with zero callers

`WelsDecodeAndConstructSlice` (`src/decoder/decode_slice.rs:5444`) and
`WelsDeblockingFilterMB` (`src/decoder/deblocking.rs:2295`) are transliterated and
**nothing calls either**:

```bash
grep -rn 'WelsDeblockingFilterMB' rust/crates/openh264-rs/src/ | grep -v 'pub fn'   # empty
```

In the reference they are on the threaded path — `decoder_core.cpp:2727` reaches
`WelsDecodeAndConstructSlice` only under `iThreadCount > 1`, and this port is
single-threaded by D3. They came in with the transliteration and were orphaned when
the threaded detour was dropped.

Found while classifying the census (T8b.A2). **S18 candidates for a later session**;
not deleted in Phase 8b, whose scope is parity rather than hygiene. Their absence
also explains why the 18 `MBPad*_c` / `PadMB*_c` kernels are classified `dead`: their
only caller is `decode_slice.cpp:1731`, inside `WelsDecodeAndConstructSlice`.

## F85 — the stub finder collapses same-named methods on two classes

`find_stub_bodies.py` keys the C++ map by bare function name and keeps the *longest*
body when a name is defined more than once. `CWelsDecoder::GetOption` (53 statements)
is therefore invisible behind `CWelsH264SVCEncoder::GetOption` (230), and anything
the decoder's version calls and the encoder's does not is un-diffable by that tool.

Measured: `python3 rust/tools/find_stub_bodies.py GetOption` reports the *encoder*'s
file on both sides.

`port_census.py` does not have this problem — it keys by name × file and prints the
class — but it only answers "present", not "equivalent". Recorded in the tool's
docstring and in `phase8b_port_census.md` §5 so the next reader does not trust a
clean row for a method name.

## F86 — a stub `ForceCodingIDR` was also aborting the process

`ForceCodingIDR` (`wels_encoder_ext.rs:629` before T8b.A4) checked `pCtx` for null and
returned 0. `encoder_ext.cpp:3046` resets `iCodingIndex`, `iFrameIndex`, `iFrameNum`,
`iPOC` and `bEncCurFrmAsIdrFlag` per dependency layer, bumps `uiIDRReqNum` and clears
`bCheckWindowStatusRefreshFlag`. So `ISVCEncoder::ForceIntraFrame(true)` reported
success and did nothing.

**The wrong frame type** was the visible half: at IDR interval 1 every frame is an IDR
anyway and the stub is invisible; at interval 2 the frame after `ForceIntraFrame(true)`
comes back `videoFrameTypeI` (3) where the reference gives `videoFrameTypeIDR` (1).

**The abort was the other half.** Under a gtest filter,
`EncodeDecodeTestAPI.GetOptionTid_AVC_NOPREFIX` produced

```text
index out of bounds: the len is 5 but the index is 5
  at src/encoder/ref_list_mgr_svc.rs:684
panicked at src/encoder/svc_base_layer_md.rs:344: the layer's reconstruction picture is bound
panic in a function that cannot unwind  ->  abort
```

`pShortRefList` is `[_; 1 + MAX_SHORT_REF_COUNT]` and `MAX_SHORT_REF_COUNT` is
`MAX_GOP_SIZE >> 1 = 4`, so `uiShortRefCount` had reached 6: the short-reference list
grew past its bound because the IDRs that would have cleared it were never coded.
**The C++ writes `pShortRefList[iRefIdx + 1]` unchecked** at
`ref_list_mgr_svc.cpp:387–391`, so the same state corrupts memory there and panics
here.

**It was reachable only after T8b.A3.** The test feeds the decoder's
`LTR_MARKING_FLAG` / `IDR_PIC_ID` / `FRAME_NUM` back into the encoder through
`LTRRecoveryRequest`; while `GetOption` returned the caller's stack garbage the
recovery path was never entered. One parity fix made a second one reachable, and the
second was an abort — **D-prio-1's thesis, measured**.

`test/api` seeds `rand()` from `time(NULL)` (`simple_test.cpp:20–24`), so it was one
run in six; `--seed=1787387872` with `--gtest_filter='*GetOption*:*DecoderVclNal*:*Engine_SVC_Switch*:*ProfileLevel*'`
pinned it. **Fixed** by T8b.A4's port of `ForceCodingIDR`: that seed and eight further
LTR-heavy runs are clean.

*What is not claimed:* that the bound is now unreachable. Only that the one
reproduction is gone. The unchecked C++ write and the port's panic are both still
there for any other state that overruns the list — a Phase 9 question.

## F87 — the decoder answers `dsOutOfMemory` where the reference decodes cleanly

`WelsRequestMem`'s third arm — same picture size, changed `num_ref_frames`
(`decoder.cpp:493–509`) — needs `IncreasePicBuff` / `DecreasePicBuff`, which this port
has never had (**F80**). T8b.A6 established that the arm is reachable and built the
asset for it: `rust/crates/openh264-rs/tests/data/f80/num_ref_change_320x192.264`,
made with the reference encoder (`rust/tools/make_numref_asset.cpp`) as 24 frames at
`iNumRefFrame = 1` followed by 24 at 4, same 320x192.

The reference decodes it in full and says what it is doing:

```text
WelsRequestMem(): memory alloc size = 320 * 192, ref list size = 3
WelsRequestMem(): memory re-alloc for no resolution change (size = 320 * 192),
                  ref list size change from 3 to 6
```

`ecref res/…/num_ref_change_320x192.264 99999999` →
`48 320x192 20c1d2ea23e46f101a9806be1bb046246f7bb9d9`.

The port emits **nothing for 16 consecutive calls** in the second half, and
`decoder_reachability_sweep.rs` — which globs `res/` — refuses the asset outright:

```text
an arm documented as unreachable was reached — that is a fact worth a finding:
  num_ref_change_320x192.264 ec=ERROR_CON_DISABLE bs=VIDEO_BITSTREAM_AVC -> dsOutOfMemory
  ... every concealment mode, both bitstream types
```

So F80 is not a latent gap: a consumer that reconfigures its encoder's reference count
gets `dsOutOfMemory` from this decoder and a full decode from the reference.

**Owner: Phase 8b session C.** The port is `IncreasePicBuff` (36 C++ statements) and
`DecreasePicBuff` (50), and in Rust it is a grow/shrink on `PicPool`'s
`safe::Pool` — whose contract today says it never grows or shrinks and whose `PicId`s
carry debug generations, so it is a change to a `src/safe/` invariant and not a
transliteration. Moving the asset into `res/` belongs to the same commit: until then
the sweep would be red for a defect nobody had fixed yet.

## F88 — one gtest row is intermittently red on the Rust link and never on the C++

`EncodeDecodeTestAPI.SetOptionECIDC_SpecificFrameChange` failed once in **seven** full
runs of the Rust link at `5478ae7e`, at

```text
test/api/decoder_ec_test.cpp:302:  EXPECT_TRUE (rv != 0);  //construction error due to data loss
```

Runs measured at the session close: three `gtest_stretch.sh --check` (165, **164**,
165) and four direct (`165 / 165 / 165 / 165`). The C++ link over the same four direct
runs: **199 / 199 / 199 / 199** — it never fails it.

`test/api`'s main seeds `rand()` from `time(NULL)` (`simple_test.cpp:20–24`) and the
fixtures draw frame content, temporal-layer counts and IDR intervals from it, so the
encoder feeds a different stream each run and whether a simulated slice loss actually
produces a construction error varies. In isolation
(`--gtest_filter='*SetOptionECIDC_SpecificFrameChange*'`) both links pass 10/10; only
the full run reaches the seed that breaks it.

**What this is and is not.** It is not test noise the port is entitled to: the C++
passes it under the same seeds, so on that seed the port's concealment path returns
success where the reference returns an error. It is rare enough that it did not appear
in the four consecutive close runs, so it is **not** a reason to allowlist the row —
allowlisting it would make `--check` red on the six runs out of seven where it passes,
which is the stale-row failure the gate exists to catch.

**Consequence for the ratchet:** `gtest_stretch.sh --check` will occasionally report
this row as `FAILING BUT NOT IN`. That is the gate working; re-run before treating it
as a regression, and record the seed (`--seed=N`, printed as `Random seed:`) if it
recurs so the divergence can be pinned. Owner: unassigned — it needs a seed that
reproduces before it can be chased.

---

# Session B (T8b.B1–…)

## F89 — a seventh listing-strategy row, invisible four runs in seven

`DecodeCrashTestAPI.DecoderCrashTest` (`decode_api_test.cpp:600`) opens with

```c
EParameterSetStrategy eCurStrategy = CONSTANT_ID;
switch (rand() % 7) {
  case 1: eCurStrategy = INCREASING_ID; break;
  case 2: eCurStrategy = SPS_LISTING; break;
  case 3: eCurStrategy = SPS_LISTING_AND_PPS_INCREASING; break;
  case 6: eCurStrategy = SPS_PPS_LISTING; break;
  default: break;                        // the initial value
}
```

Three of the seven draws select a strategy `CreateParametersetStrategy` returns `None`
for, and the test's first `InitializeExt` then fails at `decode_api_test.cpp:666`. On
the other four it passes. Nothing pinned `rand()` before T8b.B1, so the row flipped
between runs and was never allowlisted: **session A's 165/199 was an unseeded sample
that happened to draw a non-listing strategy.**

With `SEED=20260822` the first `rand()` of the run — this test is the fourth to run and
the three before it draw nothing — is `3`, i.e. `SPS_LISTING_AND_PPS_INCREASING`. The
honest tally at the pinned seed is **164/199, 35 allowlisted**, and this row leaves with
the other six when the listing strategies land.

The general point is the one S49 was written for: a suite that seeds itself from the
clock does not have a tally, it has a distribution, and a ratchet cannot be built on a
distribution.

## F90 — `DecodeParser`'s copy-out overwrites the caller's input timestamp, and its `memset` counts bytes for an `int32_t` array

Two defects in `CWelsDecoder::DecodeParser` (`welsDecoderExt.cpp:1180–1262`), both
found by transliterating it and both reproduced rather than repaired.

**1. The input timestamp is destroyed.** The copy-out is one `memcpy`:

```c
if (!pDecContext->bFramePending && pDecContext->pParserBsInfo->iNalNum) {
  memcpy (pDstInfo, pDecContext->pParserBsInfo, sizeof (SParserBsInfo));
```

`SParserBsInfo::uiInBsTimeStamp` is an **input** field — `:1226` reads it out of the
caller's struct into `pDecContext->uiTimeStamp` — and nothing anywhere writes the
decoder-side copy (`grep -rn 'pParserBsInfo->uiInBsTimeStamp' codec/` finds nothing).
So every call that completes a frame hands the caller back `uiInBsTimeStamp = 0`. It is
visible in the goldens as the `in=0` column on exactly the emitting rows:

```text
PARSE 2 rv=0x0 nal=0 lens=[]              sps=0x0     in=3 out=0
PARSE 3 rv=0x0 nal=3 lens=[13,8,2363]     sps=176x144 in=0 out=3    <-- overwritten
```

The port does the same, field by field, with a comment at the assignment. It is
observable behaviour on a documented out-parameter; a drop-in that "fixed" it would
diverge from every consumer written against the reference.

**2. `memset (pDecContext->pParserBsInfo->pNalLenInByte, 0, MAX_NAL_UNITS_IN_LAYER)`**
(`:1220`) clears `MAX_NAL_UNITS_IN_LAYER` **bytes** of an array of that many `int32_t`
— the first 32 of 130 elements. This is `find_elem_byte_confusion.py`'s exact shape, in
the reference rather than the port. It is unobservable in either tree: every slot is
written by `pNalLenInByte[iNalNum++] = …` before anything reads it, and the only reader
sums `0..iNalNum`. The port clears the whole `Vec`, which is what the statement means.

## F91 — parse-only writes into the caller's bitstream

`ParseNalHeader`'s slice-extension arm rewrites the NAL type byte **in the application's
input buffer** before copying it out (`au_parser.cpp:346–351`):

```c
if (pCurNal->sNalHeaderExt.bIdrFlag) {
  * (pSrcNal + iCurrStartByte) &= 0xE0;
  * (pSrcNal + iCurrStartByte) |= 0x05;
} else { … |= 0x01; }
pSavedData->pCurPos[4] = * (pSrcNal + iCurrStartByte);
```

`pSrcNal` traces back to `WelsDecodeBs`'s `const_cast<uint8_t*> (kpBsBuf)`
(`decoder.cpp:766`) — the buffer the caller passed to `DecodeParser`, declared `const`
on the public interface. So decoding an SVC stream in parse-only mode silently modifies
the caller's memory, and a caller that feeds the same buffer twice gets different
results the second time.

The port computes the byte and copies it (`parse_only_capture_vcl`); the caller's
bitstream is untouched. Nothing downstream reads those bytes again — `sRawData` already
holds this NAL's de-escaped copy by the time `ParseNalHeader` runs — so the two trees
agree on every output and differ only in whether the input survives.

## F92 — two unbounded writes and a length bug in the subset-SPS rewrite

`au_parser.cpp:1191–1256` re-encodes a subset SPS as a plain Main-profile SPS so that
parse-only output stays AVC-decodable. Three things in it:

1. **The writer is sized by the wrong buffer.**
   `InitBits (&sSubsetSpsBs, pBsBuf, (int32_t) (pBs->pEndBuf - pBs->pStartBuf))` gives
   the writer the length of the *source SPS's bitstream* while `pBsBuf` is a
   `SPS_PPS_BS_SIZE + 4` = 132-byte allocation. A subset SPS longer than 132 bytes —
   a long `offset_for_ref_frame` list will do it — lets the writer run past the
   allocation.
2. **`RBSP2EBSP` has no destination bound.** It writes to `pSpsBsBuf + 5` inside a
   `SPS_PPS_BS_SIZE` = 128-byte array, and escaping only ever makes the payload longer.
3. **`uiSpsBsLen` is set from the RBSP size, not the EBSP size**:
   `pSpsBs->uiSpsBsLen = (uint16_t) (sSubsetSpsBs.pCurBuf - sSubsetSpsBs.pStartBuf + 5)`
   — measured *before* the escape. A subset SPS whose rewrite needs an emulation-
   prevention byte is therefore handed to the caller one byte short per inserted
   `0x03`, i.e. truncated.

The port's `parse_only_write_subset_sps` bounds the writer by the array it writes into,
refuses a rewrite that does not fit (`dsOutOfMemory`, the reference's own code for a
parse-only allocation failure), and takes `uiSpsBsLen` from `rbsp_to_ebsp`'s return —
the escaped length, which is what the copy-out reads.

**Reachability is not established.** No `res/` asset produces a subset SPS whose rewrite
needs an escape, so (3) is a divergence the goldens do not currently exercise; it is
written the correct way because writing the truncation deliberately would need its own
justification. Owner for a reachability probe: whoever ports SVC parse-only
(`ParseOnly_General`, 8b.C then this).

## F93 — parse-only + `ERROR_CON_DISABLE` on a damaged stream: six rows disagree

`res/Error_I_P.264` is refereed by `ecref --parse-only` and its golden is checked in at
`tests/data/decoder_parseonly/Error_I_P.txt`, but it is **not** in
`decoder_parseonly_parity_test.rs`'s `ASSETS` — it is in `DIVERGING`, with the table:

```text
  row  7   ref rv=0x0                        port rv=0x1 (dsFramePending)
  row  9   ref nal=5 [13,8,5601,8827,10956]  port rv=0x4, nothing emitted
  row 13   ref rv=0x1                        port rv=0x2 (dsRefLost)
  row 14   ref rv=0x1                        port rv=0x4
  row 16   ref rv=0x2                        port rv=0x4
  total    ref 1 frame emitted               port 0
```

**Why nothing saw it before.** `DecodeParser` forces
`pParam->eEcActiveIdc = ERROR_CON_DISABLE` on every call (`welsDecoderExt.cpp:1217`).
The malformed corpus runs all 2707 of its rows with `ERROR_CON_SLICE_COPY`
(`malformed_stream_parity.rs:490`) and the conformance assets are undamaged, so
**"damaged stream, concealment disabled" had no referee at all** — in either decode
entry point. Parse-only is the messenger, not necessarily the defect.

**Not T8b.B2's.** Every write that commit adds is behind `pParam.bParseOnly`, and the
only codes it can raise are `dsOutOfMemory` (the two `MAX_ACCESS_UNIT_CAPACITY` checks)
and `dsBitstreamError` from the `SPS_PPS_BS_SIZE - 4` guard, which needs a parameter set
of 124 bytes or more where this stream's are 13 and 8. The codes above are
`dsFramePending`, `dsRefLost` and `dsBitstreamError` from pre-existing paths.

**The next experiment, named rather than guessed:** give `ecref` an `--ec=<idc>` flag
and drive the *ordinary* `DecodeFrame2` path over this asset with `ERROR_CON_DISABLE` on
both links. If that diverges too, the defect is in the decoder's error path and the
corpus needs a second concealment mode; if it does not, it is in the parse-only arm of
`DecodeFrameConstruction`.

**Answered, same session — see F96.** It diverges there too: on the ordinary
`DecodeFrame2` path with `ERROR_CON_DISABLE` this asset reports `dsBitstreamError`
where the reference reports `dsRefLost`, at two calls of seventeen, because
`InitRefPicList` returns `ERR_NONE` in the port and `ERR_INFO_REFERENCE_PIC_LOST` in
the reference. So the defect is in the decoder's error path and parse-only is the
messenger. F93's remaining rows — the emitted frame at row 9 that the port does not
produce, and the `dsFramePending` at row 7 — are what parse-only adds *on top* of
that, and they should be re-measured once F96 is fixed. Owner: unassigned, after F96.

## F94 — the SPS_PPS_LISTING structure export reads one PPS and copies 57

`CWelsParametersetSpsPpsListing::OutputCurrentStructure` (`paraset_strategy.cpp:688`):

```c
memcpy (pExistingParasetList->sPps, pCtx->pPps, MAX_PPS_COUNT * sizeof (SWelsPPS));
```

`pCtx->pPps` is **`SWelsPPS*`, a pointer to the one active PPS** — the encoder's array
is `pCtx->pPPSArray`, and every other statement in this family uses that
(`LoadPreviousPps` at `:553` writes `pPpsArray`, `UpdatePpsList` at `:576` writes
`pCtx->pPPSArray[iPpsId]`). So the copy reads `MAX_PPS_COUNT * sizeof (SWelsPPS)` bytes
starting at a single struct: 56 structs past its end.

The port copies `ctx_pps_array(pCtx)`, which is what the sentence means and what the
matching `LoadPreviousStructure` reads back. Reproducing the over-read is not an
option — there is nothing behind `pPps` to read — and it is not observable in a
correct program: what lands in `sPps[1..]` is whatever followed the active PPS, and
the only reader is `LoadPreviousPps` on the *next* `InitializeExt`, which copies it
into `pPpsArray` before `FindExistingPps` compares against `uiInUsePpsNum` entries
of it.

**Reachability of the difference is not established.** It needs an
`InitializeExt` → `OutputCurrentStructure` → `InitializeExt` → `LoadPreviousPps` round
trip under `SPS_PPS_LISTING` with more than one PPS in use, which is what a
`--reinit-at` knob in the diffharness drivers would build. T8b.B3 dropped that knob
(the brief's step 5) and left the six `ParameterSetStrategy_*` gtest rows as the
referee; they pass, but they do not hash bytes across the re-init.

## F95 — `find_stub_bodies.py` cannot see through `abi_guard!`, so every C-ABI thunk reads as a stub

Re-running the stub finder over T8b.B2's work (the brief's step 4) reports:

```text
DecodeParser   [0 Rust statements vs 37 C++]
    Rust rust/crates/openh264-rs/src/api/codec_api.rs
```

which is exactly what it said when `decoder_decode_parser_c` really *was* a stub. The
reading did not change, because the tool cannot count through the macro: every sibling
thunk answers the same way at the same commit —

```text
DecodeFrame          [0 Rust statements vs 13 C++]
DecodeFrameNoDelay   [0 Rust statements vs 12 C++]
FlushFrame           [0 Rust statements vs  7 C++]
```

— and all four have full bodies. The thunk's body is one `abi_guard!(...)`
invocation, and the tool's statement count sees a macro call.

**What this costs.** T8b.A2's alias table was added so that the C++ `DecodeParser`
would be compared against the real thunk rather than a same-named trampoline (F85's
shape). It succeeded at that and the count is still blind, so the instrument's signal
on the **twenty C-ABI entry points** — the port's whole outward surface — is zero in
both directions: it cannot see a stub there, and it cannot see a stub being fixed.
Every one of those slots needs a behavioural referee, which is the S47 rule and is why
T8b.B2 built one.

The fix is small and mechanical — count statements inside a leading `abi_guard!` /
`panic_probe!` block rather than treating the invocation as one — and it is not this
session's, because changing what the instrument reports is a measurement change and
wants its own commit. Owner: unassigned; the phase close is the natural place.

## F88 — **reproduced at seed 5**, and the assertion is not the one session A recorded

`gtest_stretch.sh --seeds=1..10` (T8b.B1's finder, both links per seed):

```text
seed 1  rust 173/199   cxx 199/199    rust-only failures: the 26 allowlisted
seed 2  rust 173/199   cxx 199/199    ditto
seed 3  rust 173/199   cxx 199/199    ditto
seed 4  rust 173/199   cxx 199/199    ditto
seed 5  rust 172/199   cxx 199/199    + EncodeDecodeTestAPI.SetOptionECIDC_SpecificFrameChange
seed 6  rust 173/199   cxx 199/199    the 26
seed 7  rust 173/199   cxx 199/199    the 26
seed 8  rust 173/199   cxx 199/199    the 26
seed 9  rust 173/199   cxx 199/199    the 26
seed 10 rust 173/199   cxx 199/199    the 26
```

**One seed in ten**, against session A's one run in seven unseeded — the same rate,
and now a name for the case rather than a rate. The C++ link is 199/199 on all ten,
which is what makes it the port's and not the suite's.

**Seed 5 is the repro**, and it is a whole-suite one: the row only fails inside the
full run, because a `--gtest_filter` changes the `rand()` sequence the fixtures draw
their content from.

**The assertion is `decoder_ec_test.cpp:302`, and that line is
`EXPECT_EQ (dstBufInfo_.iBufferStatus, 1)` — not `EXPECT_TRUE (rv != 0)`.** Session A
recorded the neighbour one line up. What the row actually reports at seed 5 is

```text
test/api/decoder_ec_test.cpp:302: Failure
Expected equality of these values:
  dstBufInfo_.iBufferStatus
    Which is: 0
  1
```

which changes what the defect is. The context (`decoder_ec_test.cpp:286-302`) is
**Frame 3, `ERROR_CON_SLICE_COPY`, no loss of its own, decoded after Frame 1 was
dropped entirely**: `rv != 0` passes — the port does report the construction error —
and then the reference *conceals and emits a frame* where the port emits nothing.

So F88 is a **concealment** divergence, not an error-reporting one: with slice copy
enabled and a damaged reference chain, `iBufferStatus` comes back 0 from this port and
1 from the reference. That it is intermittent follows from the fixture's content being
`rand()`-derived — whether Frame 3's slices are damaged *enough* to reach the arm
varies with the encode.

**Owner: unassigned, with the repro attached.** `gtest_stretch.sh --seeds=5..5` and
read `out/gtest/rust_5.log`. The next step is to instrument `DecodeFrameConstruction`'s
output decision on that access unit and compare the two links; F96's `ecref_rs` is the
shape that comparison should take, though this row needs the encoder in the loop and so
needs the gtest binary rather than a `res/` asset.

## F96 — `ERROR_CON_DISABLE` had no referee in any decode entry point, and the port diverges

F93 named an experiment. It has been run.

`ecref` grew `--ec=<idc>` and `--trace=<level>`, and `build.sh` now builds the same
program a second time against the **port's** cdylib (`ecref_rs`) — the seven exported
symbols are all it links, so this is one extra link line and no extra code. Two
programs, one source, one question each.

**The measurement**, over every one of `res/`'s 63 assets:

```text
--ec=2  (ERROR_CON_SLICE_COPY, the mode every existing referee runs)
        60 agree, 3 differ  — CABA2_SVA_B, test_scalinglist_jm (both D-poc-1, by
                              design) and Error_I_P
--ec=0  (ERROR_CON_DISABLE, the mode nothing runs)
        60 agree, 3 differ  — the same three
```

`Error_I_P.264` is the one whose disagreement is *new at `--ec=0`*, and it is codes
only — same frame count, same bytes, same `iBufferStatus` on every call, and every
`GetOption` scalar equal on every call:

```text
ref   … 0x0,0x0,0x0,0x4,0x0,0x0,0x2,0x0,…,0x4,0x4,0x0,0x2
port  … 0x0,0x0,0x0,0x4,0x0,0x0,0x4,0x0,…,0x4,0x4,0x0,0x4
                                ^^^                   ^^^
```

`0x2` is `dsRefLost`, `0x4` is `dsBitstreamError`, at calls 6 and 16 of 17.

**The arm, from the reference's own trace** (`--trace=8`, which is why that flag
exists — the default sink prints `WELS_LOG_ERROR` and above, so every `WELS_LOG_WARNING`
naming an error arm was invisible):

```text
Debug:reference picture introduced by this frame is lost during transmission! uiTId: 0
Debug:returned error from decoding:[0x433]
```

That is `decoder_core.cpp:2708-2718`: `InitRefPicList` returns
`ERR_INFO_REFERENCE_PIC_LOST`, `HandleReferenceLost` ors `dsRefLost`, and under
`ERROR_CON_DISABLE` the arm returns immediately — so `WelsDecodeSlice` never runs and
`HandleReferenceLostL0`'s `dsBitstreamError` is never reached. **In the port
`InitRefPicList` returns `ERR_NONE`**, the arm is skipped, and the slice decode fails
instead.

**The narrowing is done; the fix is not.** The divergence is inside `InitRefPicList`
(`decoder_core.rs:4220`) or one of `WelsInitRefList` / `WelsReorderRefList` /
`WelsReorderRefList2` beneath it. Two facts for whoever takes it:

* `WelsCheckAndRecoverForFutureDecoding` cannot be it: under `ERROR_CON_DISABLE` its
  whole body is skipped and it returns `ERR_NONE` in both trees.
* `WelsReorderRefList`'s three failure returns all *assign* `iErrorCode = dsNoParamSets`
  first, so a failure there would report `0x12`, not the `0x2` observed. Whatever
  returns `0x433` here does not touch `iErrorCode`.

**A second, smaller gap found on the way**: the port emits **4 of the reference's 24**
trace lines on this stream at `--trace=8`, and one of the missing ones is the
`"reference picture introduced by this frame is lost during transmission!"` at
`decoder_core.cpp:2713` — the very line that named the arm. Adding it is one statement
and it is deliberately *not* in this session's commits: it is a behaviour change on the
trace surface, and it belongs with the fix it helps find, not ahead of it.

**Relationship to F93 and F88.** F93 (parse-only on `Error_I_P`) is the same mode —
`DecodeParser` forces `ERROR_CON_DISABLE` — and its divergence is larger, so it is
this plus whatever parse-only adds. F88 is **not** this: its assertion is a
concealment-mode output, measured above.

**What this says about the corpus.** 2707 rows, all `ERROR_CON_SLICE_COPY`. The
`--ec=` axis is one flag away from being swept and would have caught this whenever it
appeared. Adding a second concealment mode to `compare_all.sh` is the obvious next
instrument and is not this session's (D-gate-3 puts sweeps at the phase close).

## F97 — the `_c` downsamplers *are* the NEON downsamplers, byte for byte; the divergence is in the dispatch **table**, not the kernels

Session C's brief named one risk that could sink the session: the tests link
`libopenh264.a`, built with assembly, so on this arm64 host `CDownsampling`
dispatches to NEON, while the port would naturally translate the `_c` kernels — and
upstream itself hedges, giving `EncoderOutputTest` rows 5 and 7 *two* golden hashes
each with the comment about "whether averaging is done vertically or horizontally
first when downsampling" (`test/api/encoder_test.cpp:163,174`).

**Measured, not reasoned.** `rust/tools/vp_kernel_probe/kernel_parity.cpp` links the
reference archive and calls each `_c` kernel and its NEON sibling over identical
input, diffing the destination window. 40 comparisons — eight frame sizes × the
`x16`/`x32` stride branches for the half-average, eight for quarter, eight for
one-third, four general-ratio:

```text
half 320x192 … 1280x720 … (both stride branches)   IDENTICAL   (16/16)
quarter 320x192 … 1280x720                          IDENTICAL   (8/8)
onethird 320x192 … 1280x720                         IDENTICAL   (8/8)
general ACC 320x192->208x128 … 1280x720->848x480    IDENTICAL   (4/4)
==> ALL COMPARED KERNELS IDENTICAL
```

So the headline risk **does not exist**: no `WELSCPUFLAG=0`, no `USE_ASM=No`
reference build, no choosing between two goldens. Port the `_c` kernels and parity
against `cxx_enc` holds as it always has.

**But the probe found the real trap one line away.** The divergence is not between a
kernel and its sibling, it is between the two *tables*. `InitDownsampleFuncs`
(`downsample.cpp:85–141`) binds the scalar table

```c
sDownsampleFunc.pfGeneralRatioChroma  = GeneralBilinearAccurateDownsampler_c;
sDownsampleFunc.pfGeneralRatioLuma    = GeneralBilinearFastDownsampler_c;      // Fast
```

and then the aarch64 arm **rebinds luma to the accurate wrapper**:

```c
sDownsampleFunc.pfGeneralRatioChroma = GeneralBilinearAccurateDownsamplerWrap_AArch64_neon;
sDownsampleFunc.pfGeneralRatioLuma   = GeneralBilinearAccurateDownsamplerWrap_AArch64_neon;  // *not* Fast
```

There is no NEON "fast" general downsampler at all, so the aarch64 table quietly
substitutes the accurate one for luma. Fast and Accurate are **not** the same
function — the same probe measures the gap:

```text
general FAST_c vs ACC_c 320x192->208x128    DIFFER (3366/26624 px)
general FAST_c vs ACC_c 1280x720->848x480   DIFFER (1485/407040 px)
general FAST_c vs ACC_c 640x360->424x240    DIFFER ( 361/101760 px)
```

A port that transliterated `InitDownsampleFuncs`'s *scalar* table — which is the
obvious thing to do, and what the brief describes — would use Fast for luma and
diverge on every non-dyadic ratio, while matching on every dyadic one. That is the
worst shape a bug can have: invisible in the gate configuration, wrong later.

**Decision (step 0): port the `_c` kernels, bind them the way the aarch64 table binds
them.** Recorded at `processing/downsample.rs` and in the `dl` preset's comment.

## F98 — the arm of `CDownsampling::Process` that actually runs is the *other* one, and it makes two of the five kernels dead code

`Process` (`downsample.cpp:144`) has two arms:

```c
if ((iSrcWidthY >> 1) > MAX_SAMPLE_WIDTH || (iSrcHeightY >> 1) > MAX_SAMPLE_HEIGHT || m_bNoSampleBuffer) {
    // arm 1: single pass — half / quarter / onethird / general, picked by ratio
} else {
    // arm 2: a do-while that halves repeatedly through m_pSampleBuffer
}
```

`m_bNoSampleBuffer` reads like "this object has no sample buffer", and the brief took
it that way. It is the opposite: it is `AllocateSampleBuffer()`'s return value, and
that function returns `false` on **success** and `true` only when a `WelsMalloc`
failed (`downsample.cpp:49,56–72`). So on any host where the four allocations
succeed, `m_bNoSampleBuffer == false`, and with `MAX_SAMPLE_WIDTH = 1920` the size
test needs a source wider than 3840 to fire. **Arm 2 is the normal path. Arm 1 is the
out-of-memory fallback.**

The consequence is not cosmetic. Arm 2 never calls `pfQuarterDownsampler` or
`pfOneThirdDownsampler`; it reaches the target by repeated halving and finishes with
either an exact half-average or the general-ratio kernel. Measured against the real
class (`rust/tools/vp_kernel_probe/dispatch_model.cpp`, which constructs a
`CDownsampling` and diffs `Process`'s output against candidate kernels):

```text
4:1   640x384 -> 160x96    single-pass quarter = no    two cascaded half-averages = MATCH
4:1  1280x720 -> 320x180   single-pass quarter = no    two cascaded half-averages = MATCH
gen   320x192 -> 208x128   general-Fast        = no    general-Accurate           = MATCH
3:1   960x576 -> 320x192   single-pass onethird= no    (halve to 480x288, then general)
```

So `DyadicBilinearQuarterDownsampler_c` and `DyadicBilinearOneThirdDownsampler_c` are
**unreachable** in the configuration the tests run, and a port that dispatched 4:1 to
the quarter kernel — as the brief instructs — would be wrong on exactly the ratio row
7 exercises.

**The validated model.** `dispatch_model.cpp` carries a plain-C++ transcription of arm
2 plus the two kernels it can reach (`DyadicBilinearDownsampler_c` and
`GeneralBilinearAccurateDownsampler_c`), and diffs it against `CDownsampling::Process`
over 19 source→destination pairs covering 2:1, 4:1, 8:1, 3:1 and four general ratios,
all three planes:

```text
==> MODEL MATCHES THE REFERENCE ON EVERY CASE   (19/19, Y+U+V)
```

That model — not the source reading — is what `processing::downsample` is ported from.

Two details in it that a transcription drops silently and the probe would have caught:

* `DownsampleHalfAverage` (`downsample.cpp:279`) does **not** pass the source width.
  It passes `WELS_ALIGN(iSrcWidth & ~1, 32)` when the source stride is 32-aligned and
  `WELS_ALIGN(…, 16)` otherwise, so the kernel writes *past* the destination's nominal
  width into the padding, and how far depends on the stride. Both branches land on the
  same `_c` kernel; only the width differs.
* The intermediate strides inside the loop are `WELS_ALIGN(iHalfSrcWidth, 32)` for
  luma and `WELS_ALIGN(iHalfSrcWidth >> 1, 32)` for chroma — recomputed each pass, not
  inherited from the destination.

## F99 — denoise has no NEON on aarch64, so it carries none of F97's risk

`CDenoiser::InitDenoiseFunc` (`denoise.cpp:55–65`) has exactly one non-scalar arm and
it is `#if defined(X86_ASM)`. There is no NEON denoise in the tree, and `nm` on the
reference archive agrees — the only denoise symbols it exports are
`BilateralLumaFilter8_c`, `WaverageChromaFilter8_c` and `Gauss3x3Filter`. On this host
the reference runs the same scalar filters the port will, so `EncoderOutputTest/4`
needs no kernel-selection decision at all.
