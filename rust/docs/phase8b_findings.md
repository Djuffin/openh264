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
