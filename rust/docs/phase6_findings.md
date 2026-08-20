# Phase 6 findings

Findings opened during Phase 6 (the encoder structural rewrite). Numbering
continues `phase5_findings.md`, which ends at F56.

---

## F57 — `MvdCostInit`'s second cursor walks off the end of the MVD cost table

**Status: FIXED 2026-08-18 (Phase 6 session A), by accommodation rather than by
repair — F14's move, for F14's reason.** Found by the encoder aliasing probe
(`encoder_initialisation_runs_under_the_aliasing_checker`) on the first run in
which it got past F13's site, which is the finding's real subject: this had been
in the tree since the port began and no gate could see it.

### What it is

`RequestMemorySvc` allocates the inter MVD cost table as `52 * stride * 2` bytes,
where `stride = 1 + 2 * kuiMvdInterTableSize` (`encoder_ext.rs`). `MvdCostInit`
(`md.rs`) fills it with two cursors, one per half of each row:

* `pNegMvd` starts at the table's base, writes `kiSz` entries, writes one more,
  then advances `kiSz + 1` — a full stride per row. After the 52nd row it sits
  **exactly one past the end**, which `offset` permits.
* `pPosMvd` starts at `base + (kiSz + 1)` and advances by the same stride. After
  the 52nd row it sits `(kiSz + 1)` elements **beyond** the end — 1042 bytes on
  the probe's configuration.

That last bump is computed and never dereferenced. Forming it is UB in Rust and
in C alike, and the C++ upstream forms the identical pointer: the port did not
introduce this, it transliterated it.

### Why no gate has ever seen it

The same reason F14 survived: nothing observable comes of it. The value is
computed into a register and discarded, so byte-exact sweeps, conformance and
both benches agree with it. Only Miri can see it, and **the encoder had no
Miri-covered path that reached encoder initialisation** until this session.

### The fix

`52 * kuiMvdCacheAlignedSize + 2 * ((stride >> 1) + 1)` — the smallest allocation
that makes the arithmetic legal, which is F14's `2 * 256 + 16` again. The extra
bytes are never read, never written, and never addressed except by the one bump
this exists to keep in bounds, so no encoded byte can move. Deleting the term
restores the UB and the live encoder probe catches it.

**S12 met in production for the second time**, and the rule's own sentence covers
it: a raw kernel's pointer footprint is bigger than its write footprint. F14 was
the first, in `svc_encode_slice.rs`; this is the second, and both were found by
the same instrument the first time it was pointed at the code.

---

## F58 — the encoder reads a never-written reference picture's visible luma

**Status: FIXED 2026-08-18 (Phase 6 session A), by accommodation.** Found by the
encoder probe at `processing/vaacalc.rs:307`, three fixes after F57.

### What it is

`AllocPicture` (`wels_preprocess.rs`) takes the picture's sample buffer from
`WelsMalloc`, **not** `WelsMallocz` — faithfully, because the C++ does the same
(`codec/encoder/core/src/picture_handle.cpp:76`). On the **first** frame,
`AnalyzeSpatialPic` calls `VaaCalculation` with a reference picture that nothing
has written yet (`wels_preprocess.cpp:289` does this too), and `VAACalcSad` reads
it. Miri reports the read at `pData[0] + 16` — **row 0 of the visible luma**, not
padding — of an 18,432-byte buffer that is uninitialised end to end.

Reading uninitialised memory is indeterminate-but-tolerated for `unsigned char`
in C and **Undefined Behaviour in Rust**. This is the class where a faithful
transliteration cannot stay faithful: the port has to choose a value.

### The fix, and why it cannot move a byte in practice

`WelsMallocz` for `pPic->pBuffer`. Zeroing is the smallest thing that makes the
read defined, and it is what both implementations observe anyway: a fresh 18 KB
`malloc` is served from zero pages, which is why 341/341 has always agreed. The
port becomes deterministic where the C++ is merely lucky, which is the strictly
better half of D-fid-1 — and the sweeps are what say so rather than this
paragraph.

### What it says about the instrument

F58 is not an aliasing defect at all — it is an initialisation defect that only a
whole-program interpreter can see. The probe that found it was built to find
aliasing. **The value of a Miri probe is not the class of bug it was aimed at**,
which is the third time this project has learned that (F1 behind the release
segfault, F14 behind F13's skip, and now this behind F57).

---

## F59 — the IDCT-reconstruction shims built two references over one span: the inter reconstruction is in place

**Status: FIXED 2026-08-18 (Phase 6 session B), by an in-place kernel.** Found by
the encode-path probe (`encode_loop_runs_over_a_macroblock_grid_under_the_aliasing_checker`)
the first time it reached a P frame — the eighth red of the walk, and the one that
is a new class rather than a recurrence.

### What it is

`WelsIDctT4Rec_c` / `WelsIDctFourT4Rec_c` (`svc_encode_mb.rs`) are Phase 2's shims
onto `decode_mb_aux::idct_t4_rec(rec: &mut PlaneCursorMut, pred: &PlaneCursor, dct)`.
They build `rec` with `from_raw_parts_mut(pRec, ..)` and `pred` with
`from_raw_parts(pPred, ..)`, and their doc said the two spans are **disjoint** at
every caller — naming `svc_encode_slice.rs:1039` as one of them. That caller is
`OutputPMbWithoutConstructCsRsNoCopy`, the inter-macroblock reconstruction, and it
passes **`pDecY` as both `pRec` and `pPred`** — exactly as the C++ does
(`WelsIDctT4RecOnMb (pDecY, kiDecStrideLuma, pDecY, kiDecStrideLuma, pScaledTcoeff)`):
the residual is added to the prediction *in place*.

Element-wise that is well defined (each sample is read, then overwritten). As a
`&mut [u8]` and a `&[u8]` over the same bytes it is UB before the kernel runs a
single add: Miri reports the `Unique` for `rec` "invalidated by a SharedReadOnly
retag" at the very next line, and the first write through `rec` dies.

### Why no gate has ever seen it

The kernel reads `pred[y][x]` before it writes `rec[y][x]`, so the bytes it produces
are the C++'s bytes; 341/341 has held over it since Phase 2. Only the aliasing
checker can see a contract violated by two references that happen to be used in a
benign order — and the encoder had no probe that reached a P frame until this
session's settlements let this one through.

### The fix

`idct_t4_rec_in_place(rec, dct)` / `idct_four_t4_rec_in_place(rec, dct)` beside the
two-plane kernels, sharing the transform core (`idct_t4_residual`) so the arithmetic
exists once; the in-place body reads the sample where the two-plane body reads
`pred` and writes it where that body writes `rec`. The shims dispatch on
`pRec == pPred && iStride == iPredStride` and take one cursor; the doc now says which
callers alias and that no other overlap is legal. Bit-exact by construction and by
the sweeps.

**The instrument, again**: the shim's own precondition named the offending caller as
a witness for the opposite claim, and no reader caught it in four phases. A
documented aliasing contract is a claim; the probe is the check.

---

## F60 — `FrameBsRealloc` moves the NAL-length array and never re-aims the layers at it

**Status: FIXED 2026-08-19 (Phase 6 session D, face 0), by transcribing the loop
the port dropped.** Found by the dynamic-slice probe
(`encode_loop_runs_over_size_limited_dynamic_slices_under_the_aliasing_checker`)
on its first execution — the third encoder probe, and the first thing it did.

### What it is

`FrameBsRealloc` (`svc_encode_slice.rs`) grows the frame's NAL bookkeeping when a
frame codes more slices than the list was sized for. The C++
(`svc_encode_slice.cpp:1562`) allocates a bigger `pOut->pNalLen`, copies, frees the
old one — and then closes with the part that matters:

```c
  pLBI1 = &pFrameBsInfo->sLayerInfo[0];
  pLBI1->pNalLengthInByte = pCtx->pOut->pNalLen;
  while (pLBI1 != pLayerBsInfo) {
    pLBI2 = pLBI1;
    ++ pLBI1;
    pLBI1->pNalLengthInByte = pLBI2->pNalLengthInByte + pLBI2->iNalCount;
  }
```

Every `SLayerBSInfo` handed to the caller carries a cursor **into** that array —
stamped at frame start (`wels_encoder_ext.rs:617`, `encoder_ext.rs:2077`, `:3113`)
and advanced per layer by `.add(iCountNal)`. The reallocation moves the array, so
without that loop every one of those cursors names the freed block. The port kept
the two allocations (as `Vec::resize`, T3-era) and **dropped the loop entirely**.

The result is a use-after-free on the encoder's ordinary output path: slice sizes
are written through the stale cursor, the api sums frame statistics through it
(`wels_encoder_ext.rs:1851`), and every C caller reads `pNalLengthInByte` to find
its NALs.

### What it takes to reach it, and why no gate had

`iMaxSliceNum` opens at `GetInitialSliceNum`'s answer, which for
`SM_SIZELIMITED_SLICE` is `AVERSLICENUM_CONSTRAINT` = `MAX_SLICES_NUM` = **35**;
`WelsCodeOnePicPartition` reallocates only when `iSliceIdx >= iMaxSliceNum -
iActiveThreadsNum`. So a frame must code **35 slices** — and the diffharness's two
size-limited configurations (`3 1500` and `3 600`) at the sweep's fixed `qp=26`
code at most nine on the largest clip. **The path is 341/341-green because no
configuration in the sweep reaches it**, not because it works. The three earlier
Miri probes each encode one slice a frame and never call `FrameBsRealloc` at all.

### Measured, both directions

Against the C++ encoder through `tools/diffharness/compare.sh`, `st`,
single-threaded, `res/CiscoVT2people_320x192_12fps.yuv`, 5 frames, CAVLC,
`rc=-1`, twelve configurations:

| constraint | qp 6 | qp 12 | qp 20 | qp 26 |
|---|---|---|---|---|
| 401 | **SIGBUS** (C++ 165576, Rust 0) | **SIGBUS** | DIFFER 48171 / 39750 | DIFFER 20062 / 11540 |
| 600 | **SIGBUS** | DIFFER 108546 / 94390 | DIFFER 47025 / 31735 | identical |
| 1500 | identical | identical | identical | identical |

Seven divergences and three hard crashes, in **both** build profiles, in `st` — S14
step 3's "anything outside F3's signature is real". After the fix **all twelve are
`BYTE-IDENTICAL`** and every stream decodes to a full 460800-byte YUV.

The probe's own reading, at 128x128 with a 401-byte constraint: the summed NAL
lengths read **686,479,506** against an `iFrameSizeInBytes` of **10,244** with the
re-stamp deleted, and **10,244 / 10,244** with it restored.

### The fix, and the spelling it took two goes to get right

The C++'s loop, walking **from `pLayerBsInfo` backwards** to `sLayerInfo[0]` rather
than forwards from `sLayerInfo[0]`. The first spelling walked forwards through
`addr_of_mut!((*pFrameBsInfo).sLayerInfo[i])` and the probe went red on it
immediately: `WelsEncoderEncodeExt` builds its layer cursor once as
`(*pFbi).sLayerInfo.as_mut_ptr()`, which retags the **whole array**, and
`addr_of_mut!` creates no retag at all — so those stores went through the parent's
tag and popped the array-wide child the caller was still reading through. Walking
back from `pLayerBsInfo` keeps every store inside that tag. **S28's rule in its
other direction: derive from the root the consumers share.** The layer index comes
from `offset_from` against the array base, which touches no memory.

The probe carries `bytes == frame_size` as its covering assertion — two independent
accountings of one frame's bytes, of which only the first reads through the array
this function moves.

---

## F61 — the multi-threaded slice-bank growth never re-stamps the layer's slice list

**Status: OPEN, owned by Phase 7 (`iMultipleThreadIdc > 1`).** Recorded, not fixed,
at Phase 6 session D's face 1c — the face that made `ppSliceInLayer` a list of
positions instead of a list of pointers, which removes the class this finding
describes but not the ordering defect underneath it.

### What it is

A layer's slice list is one entry per slice in coding order; each entry names a
slice inside `sSliceBufferInfo[bank].pSliceBuffer`. When a bank runs out, it is
**grown by reallocation**: `ReallocateSliceList` allocates a new block, copies the
old slices in, and frees the old block.

The single-threaded path follows through. `ReallocSliceBuffer` grows bank 0, calls
`ExtendLayerBuffer` for the list itself, and then **re-fills every entry** from the
banks' new base pointers.

The multi-threaded path does not. `ReallocateSliceInThread` grows bank
`KiSlcBuffIdx`, stores the new `pSliceBuffer` and the new `iMaxSliceNum` — and
returns. Every list entry naming that bank still holds a pointer into the block
just freed, and stays that way until `ReOrderSliceInLayer` rebuilds the list at
frame end.

**The C++ has the same shape**, checked line by line: `ReallocateSliceInThread`
(`svc_encode_slice.cpp:1325`) takes `SSlice*& pSliceList`, so the bank pointer is
updated in place and nothing else is, and `ppSliceInLayer` is untouched. This is a
shared latent defect, not a port divergence.

### Why it is not fixed here

Phase 6 converts *slice state*; Phase 7 rewrites *claiming*, and the window between
the grow and the frame-end reorder is claiming's (`phase6.md` §3, F12/P10). Fixing
it here would mean deciding what an MT worker may observe mid-frame, which is
exactly the question Phase 7 owns.

### What session D changed about it

Under the position spelling (`SliceIdx { bank, offset }`, T6.D4) a stale entry is no
longer a **dangling pointer** — it is a correct (bank, offset) pair resolved against
whatever `pSliceBuffer` currently holds, so a read through it lands inside the live
bank. The *ordering* question survives: between the grow and the reorder the entry
may name a slice the other thread has not written yet. **The index spelling removes
the memory-safety half of the class and leaves the synchronisation half**, which is
the half Phase 7 is for.

---

## F62 — `ParamTranscode` drops the caller's `iLTRRefNum`, and no gate could see it

*Phase 6 session F, 2026-08-20, found while checking the long-term-reference
paths' reachability before the picture-id flip.*

`param_svc.h:384` is

```cpp
iLTRRefNum = (pCodingParam.bEnableLongTermReference ? pCodingParam.iLTRRefNum : 0);
```

and the port's `SWelsSvcCodingParam::ParamTranscode` had transcribed it as

```rust
self.iLTRRefNum = if pCodingParam.bEnableLongTermReference { 0 } else { 0 };
```

— both arms zero, the caller's value discarded. It is a **transcription defect,
masked**: every init path that reaches `ParamTranscode` also reaches
`WelsCheckNumRefSetting` (`au_set.rs`, `au_set.cpp:92`), which overwrites the
field with `LONG_TERM_REF_NUM` (2 for camera content, 4 for screen) whenever LTR
is on and zeroes it whenever LTR is off. So the two implementations agree at
every point either of them is read, and the new `ltr` preset reads **16/16
byte-identical with the defect in place and 16/16 with it fixed**. Fixed as a
transcription rather than as a behaviour change, with that equality measured
both ways.

**What is worth carrying forward is not the line, it is why nothing found it.**
Both differential drivers hard-coded `bEnableLongTermReference = false`, so *the
entire long-term-reference subsystem* — `LTRMarkProcess`, `DeleteInvalidLTR`,
`DeleteLTRFromLongList`, `HandleLTRMarkFeedback`, `FilterLTRMarkingFeedback`,
`FilterLTRRecoveryRequest`, `WelsBuildRefList`'s long-reference arm,
`SetRefMbType`'s long half and every `pLongRefList` shift — had **no byte
coverage of any kind**, in any preset, in any profile. That is F60's shape
exactly (a path the sweep cannot reach is a path a refactor can break silently),
and it was found the same way: by asking what the harness *can* express before
converting the code, rather than after.

The `ltr` preset closes it — see `sweep.sh`. Two knobs were needed, not one:
`ltr` alone reaches marking and the list shifts, but **the feedback packets are
what unlock the rest**. Without `ENCODER_LTR_MARKING_FEEDBACK` the mark is never
confirmed, so `DeleteLTRFromLongList` never runs; without
`ENCODER_LTR_RECOVERY_REQUEST` `bReceivedT0LostFlag` is never set, so
`WelsBuildRefList` never takes its long arm. Measured entry counts over 72
frames at 320x192, `gop=0`:

| probe | fb=0 | fb=1 | fb=2 | fb=3 |
|---|---|---|---|---|
| `LTRMarkProcess` | 72 | 72 | 72 | 72 |
| `DeleteInvalidLTR` | 71 | 71 | 71 | 70 |
| `pLongRefList` shift | 1 | 14 | 1 | 3 |
| `DeleteLTRFromLongList` | 0 | **13** | 0 | **3** |
| `WelsBuildRefList` long arm | 0 | 0 | **17** | **15** |

and the four values produce four *different* streams (223062 / 223075 / 229470 /
236572 bytes), so the axis carries signal rather than repeating one encode four
times. The feedback has to quote the encoder's own `uiIdrPicId` back or
`FilterLTRMarkingFeedback` drops the packet; both drivers count coded IDR frames
to reconstruct it, which is deterministic and identical on both sides.

**Still unreached, and named rather than claimed**: `LTRMarkProcessScreen`,
`WelsUpdateRefListScreen`, `WelsBuildRefListScreen`,
`WelsMarkMMCORefInfoScreen` and `UpdateSrcPicListLosslessScreenRefSelectionWithLtr`
are the screen-content half, fenced dormant under D-scr-1 and refused by
`ParamValidationExt` for a lossy link anyway.

---

## F52's six — the Phase 6 close (adjudicated by reading, 2026-08-18, session B)

`phase5_findings.md`'s F52 repaired `tools/find_shadowing_stubs.py` and left the
six encoder-side names it then printed for Phase 6 to adjudicate before converting
anything they touch (`phase6.md` §3). Each was adjudicated by **opening both lines
the sweep names** — the "trivial" definition and the "substantial" one — and none
is F43's shape:

| name | trivial | substantial | reading |
|---|---|---|---|
| `Uninit` | `wels_task_management.rs:662` | `:748`, `common/wels_thread_pool.rs:688` | `unsafe fn Uninit(&mut self);` — a trait method **declaration** in `IWelsTaskManage`. There is no body to shadow with; `:748` is `CWelsTaskManageBase`'s impl and `:688` is `CWelsThreadPool::Uninit`, a different type. |
| `InitFrame` | `wels_task_management.rs:663` | `:926` | same trait, declaration; `:926` is the one impl. |
| `ExecuteTasks` | `wels_task_management.rs:664` | `:941`, `:1010` | same trait, declaration; the two impls are the C++ base class and `CWelsTaskManageOne`'s override, both real. |
| `OnTaskStop` | `common/wels_thread_pool.rs:97` | `:442` | declaration in `IWelsTaskThreadSink` (five-line signature ending in `;`); `:442` is the impl. |
| `WelsRcPostFrameSkipping` | `rc.rs:1860` (`false`) | `rc.rs:651` | the free `extern "C" fn` is the **faithful** port of `ratectl.cpp:1015` — `//TODO: put in the decision of rate-control` / `return false;` — and `:651` is T4b's `SWelsRcFunc::WelsRcPostFrameSkipping`, the `RCMode` dispatcher that *calls* it in the two bitrate arms and returns `false` itself in the others. Caller and callee, not stub and body. |
| `push_back` | `common/wels_thread_pool.rs:131` | `wels_task_management.rs:578` | `CWelsList<T>::push_back` and `CWelsTaskList::push_back` — two methods on two types, resolved by receiver; no shadowing is possible. |

**The instrument learns the first four.** A `fn` whose signature ends in `;`
before any `{` is a declaration, not an empty body: the sweep had scanned forward
from the declaration to the *next* item's brace and scored the nothing in between
as a trivial body. `find_shadowing_stubs.py` now skips declarations, with the
reason at the line, and carries a `--self-test` that proves the original F52 stub
shape (`fn X(a, b, c, d) -> bool { true }` across five signature lines, beside a
real `X`) **still prints** while a declaration beside a real body does not. Sweep
count at `ef8bf25b`, measured: **22 candidate names before, 18 after** — the four
declarations gone, nothing else moved (the brief's "21 → 17" was a lead — S24: the
code is identical between the commit it cites and `HEAD`). The self-test was run
against three deliberately broken copies before the claim was written: the
declaration filter removed (the pre-session tool) fails on `Uninit`; the body
counted from the `fn` line (the pre-F52 tool) fails on the stub; a *contains* `;`
filter fails on the array-type signature — all three FAIL, the shipped tool
PASSes. `WelsRcPostFrameSkipping` and `push_back` remain in
the output by design: the tool prints candidates, not verdicts, and a free
function returning a constant is F43 until read — which it now has been.

**F52 is closed.** `phase5.md`'s open-findings list and the plan's §0 row say so.

---

## F13's remaining production site — closed

Not a new finding; recorded here because Phase 6 is where it closed.
`phase2_findings.md`'s F13 named four sites and left one open for "Phase 6
(encoder context restructuring)": `InitDqLayers` taking `&mut ...sSliceArgument`
while a live pointer into the same layer was in scope. **The encoder probe
reproduced it exactly on its first execution**, at `encoder_ext.rs:822`, with
`svc_encode_slice.rs`'s `InitSliceInLayer` as the invalidator — a cross-module
pair, which is why no single-module reading had ever found it.

It closed as a family rather than a site, S29's spelling at **20** derivations of
`&mut (*<raw>).sSpatialLayers[i]` / `.sDependencyLayers[i]` across
`encoder_ext.rs`, `paraset_strategy.rs`, `ref_list_mgr_svc.rs`,
`encoder_context.rs`, `wels_encoder_ext.rs` and `wels_preprocess.rs`. See the
session A log entry for the enumeration.

**The `--skip encoder_ext` line is deleted (session B, 2026-08-18).** Session A
left it as a test-name filter over "the `encoder_ext` unit tests' own backlog";
session B measured the backlog: the filter matched exactly two tests
(`request_memory_svc_builds_the_parameter_sets`,
`request_memory_svc_builds_the_dq_layers` — `cargo test --lib -- --list | grep
encoder_ext`, and no `wels_encoder_ext` test exists to be caught by the
substring), and both ran green under the `--lib` step's flags
(`MIRIFLAGS=-Zmiri-ignore-leaks`, **2 passed / 0 failed**, Miri clock 16.92s).
There was no backlog behind the skip; S15's clause applied and the line went
with the finding it named. F13 has no open site anywhere in the tree.
