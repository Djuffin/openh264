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
`DecodeFrameConstruction`. Owner: unassigned.
