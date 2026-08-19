# Phase 6, session D — the layer owns, and the slice banks with it

Five faces, in order: **the dynamic-slice probe** → **the layer's aliases become
ids** → **the layer is `Box`-built and owns** → **the slice banks own** → **the
span**. The recipe is session C's, one level up: **a struct built by
`WelsMallocz` cannot hold an owned container (S21), so the construction changes
first and the ownership follows.** `SDqLayer` is reached only through a *pointer*
in the zeroed context (`ppDqLayerList[i]`), which is exactly T3.6's `pOut`
precedent — the zeroing never reaches its fields, so a `Box`-built layer may own
`Vec`/`MbArray` where an inline-in-a-zeroed-block struct may not.

Governing: [`phase6.md`](phase6.md) §1–§3, §6; **S18**, **S21**, **S24**, **S28**,
**S29**, **S31**, **S32**, **S33**, **S34**; **D-fid-1**, **D-par-1**, **D-perf-4**,
**D-perf-6**. Counts at `085e2e41` over `src/encoder` unless stated; re-grep at
each face's open.

## The finish rule

**No hand-off.** The session ends when §7 reads met, or at a blocker only the
steward can clear, named. A size is never a stop (S31). Questions take the decision
ladder. **All three encoder probes are live** — run them at every face's close; a
red is a finding (fix, or park with an owner, red-before/green-after observed).
Byte-exactness is the definition: `gates.sh family` at every face that touched
production code. **Drop-from-the-end is authorised at one place only** and it is
named in §6.

## 0. Start

1. Commit the inherited doc tail. Open per **S27** (session C closed `exit`
   `OVERALL: PASS` 13/0/1): build both profiles + `--all-targets`, tests
   (485 / 479 / 20), ratchet, census (58).
2. Span base **`085e2e41`** — build and stash its benches now, before the tree moves.
3. S33 on every number; a per-face breadcrumb in the log.

## 1. Face 0 — the dynamic-slice probe, before anything converts

`SM_SIZELIMITED_SLICE` is the encoder's largest unprobed surface and the phase's
first rule says the probe precedes the conversion (F47). What it reaches that
nothing under Miri does today: `WelsMdInterMbLoopOverDynamicSlice`
(`svc_encode_slice.rs:1581`), the CAVLC/CABAC stash-and-rollback pair
(`StashMBStatus`/`StashPopMBStatus`, `wels_func_ptr_def.rs:234`, `:259`),
`pDynamicBsBuffer` (`encoder_ext.rs:1116`), `CalculateNewSliceNum` →
`ReallocSliceBuffer` → `ExtendLayerBuffer` → `ReOrderSliceInLayer` — **the exact
machinery faces 2 and 3 rewrite.**

1. **It runs single-threaded** — settled by reading: `bThreadSlcBufferFlag` and
   `bSliceBsBufferFlag` both require `iMultipleThreadIdc > 1`
   (`svc_encode_slice.rs:2449`, `:2452`), and the `st` sweep preset already encodes
   `sm=3` at constraints 1500 and 600 with one thread. So the probe stays out of
   Phase 7's way: `iMultipleThreadIdc = 1`, `uiSliceMode = SM_SIZELIMITED_SLICE`,
   `uiSliceSizeConstraint` small enough to force a split.
2. Add `slice_mode`/`slice_constraint` to `EncoderProbeOptions`
   (`api/codec_api.rs:2433`) — defaults leave all three existing probes unchanged —
   and one test beside them.
3. **Non-vacuity, asserted and measured, not assumed**: more than one slice in at
   least one frame. `drive_encoder_over` returns `EncodedFrame { kind, bytes }`
   today; extend it with the frame's NAL count (`SFrameBSInfo`'s
   `sLayerInfo[..].iNalCount`) and assert ≥ 2 for a coded frame — then **measure
   the constraint that produces it** rather than picking one. A 48×32 picture is
   six macroblocks; if no constraint splits it, raise the geometry until one does
   and record the size you settled on. **A probe that encodes one slice covers
   nothing this face exists for** (F34's lesson, in this face's currency).
4. Reaching `ReallocSliceBuffer` at all is the point: assert it ran (a counter, a
   `debug_assert` on `iMaxSliceNum` growing, or the coded slice count exceeding
   `GetInitialSliceNum`'s initial value — pick one and say which). If the probe
   cannot reach the realloc path, **say so with the number that shows it** and
   name it for the session that can.
5. Record its solo cost beside the other three (session C: 79.02 s + 24.96 s;
   S32 says probe count is the budget). Its name read out of the `--lib` output
   (F17/F18). Reds are findings — session C's second probe found two on its first
   execution, session B's found eight.

## 2. Face 1 — the layer's stored aliases become ids

Before the layer can own anything, the pointers *it* stores into other layers and
into its own slice banks become indices (the ordering rule; S29's escaping class).

- **`pRefLayer: *mut SDqLayer`** (`svc_encode_slice.rs:435`, 9 sites) →
  `Option<LayerIdx>` (a `u8`/`i32` index into `ppDqLayerList`). **S34 is clean
  here, measured**: `WelsSwapDqLayers` (`encoder_ext.rs:2038`) reassigns
  `pCurDqLayer` and stamps the outgoing layer's index — it never permutes
  `ppDqLayerList`, and no `swap`/`rotate`/`retain`/`remove`/`sort`/`drain` touches
  that list anywhere. Confirm by grep at the face's open, then convert.
  `bBaseLayerAvailableFlag = !pRefLayer.is_null()` (`:2229`) becomes `.is_some()`;
  `GetRefMb` (`svc_mode_decision.rs:1403`) resolves the index against
  `ppDqLayerList`. **F56's trap**: `Option<LayerIdx>` over a plain integer has no
  niche, so it is *not* valid at all-zero the way the raw null was — which is why
  this conversion lands **after** face 2 makes the layer `Box`-built, or carries
  an explicit `None` in the constructor. State which order you took and why.
- **`ppSliceInLayer: *mut *mut SSlice`** (`svc_encode_slice.rs:394`, 49 sites over
  seven files) → `Vec<SliceIdx>` where `SliceIdx` names *(bank, offset)* — the
  bank being `sSliceBufferInfo[i]`. **This is the ordering rule's whole point
  here**: `ReallocateSliceList` (`:2542`) reallocates a bank and frees the old
  block, so every `ppSliceInLayer` entry into that bank is dangling until
  something re-stamps it — and `ReOrderSliceInLayer` (`:2758`) permutes the
  *pointer array* while the banks stay put. An index survives both. Under the
  pointer spelling the single-threaded path re-stamps (`ReallocSliceBuffer` →
  `ExtendLayerBuffer` → the loop at `:2733`) and **the multi-threaded path does
  not** — `ReallocateSliceInThread` (`:2640`) updates `sSliceBufferInfo[..].pSliceBuffer`
  and nothing else, leaving `ppSliceInLayer` stale until frame-end reordering.
  **Record that as a finding with its owner (Phase 7, `iMultipleThreadIdc > 1`) —
  do not fix the MT path here**; note whether the C++ has the same shape. The index
  spelling removes the class rather than the instance.
- **`pFeatureSearchPreparation: *mut SFeatureSearchPreparation`**
  (`svc_encode_slice.rs:433`, 11 sites): the only writer is `null_mut()`
  (`encoder_ext.rs:830`) and screen content returns `ENC_RETURN_UNSUPPORTED_PARA`
  two lines above it, so `RequestFeatureSearchPreparation`
  (`svc_motion_estimate.rs:1580`) has no reachable caller. **S18 — delete the
  field, the guard at `:1554`, and the dead function**; confirm by strip-and-build
  rather than by this paragraph.

## 3. Face 2 — the layer is `Box`-built, and then it owns

1. **The construction first.** `InitDqLayers`' `WelsMallocz(size_of::<SDqLayer>())`
   (`encoder_ext.rs:779`) becomes `Box::into_raw(Box::new(SDqLayer::new(…)))`;
   `FreeDqLayer` (`:1580`) becomes `drop(Box::from_raw(…))` with its member frees
   deleted as each member becomes owned. **`SDqLayer::default()` is
   `mem::zeroed()`** (`svc_encode_slice.rs:437`) and has two test users
   (`slice_multi_threading.rs:990`, `wels_task_management.rs:1106`) — it becomes a
   real constructor in the same commit, because a zeroed `Vec` field is UB the
   moment it drops (S21, and T5b's shells recipe: write out what each field's zero
   *meant*). De-`repr(C)` when a field stops being C-shaped, and **delete
   `assert_size!(SDqLayer, 512)` in that same commit** (phase6.md §5's rule);
   `assert_size!(SSliceCtx, 32)` and `assert_size!(SSliceBufferInfo, 16)` likewise
   when their turn comes.
2. **The MB list — one allocation becomes one `MbArray` per layer.** `InitMbListD`
   (`encoder_ext.rs:629`) allocates **one flat block for every layer** and slices it
   by cumulative `iMbSize`, then hands each layer its slice as `sMbDataP` and
   stores the same pointers in `ppMbListD`. So `sMbDataP` is not a carrier and
   `ppMbListD` is not a second owner — they are one allocation, cut disjointly and
   contiguously, each cut exactly `iMbWidth * iMbHeight`. **The layer owns
   `MbArray<SMB>`** (`safe/mb_grid.rs`, legal now that the host is `Box`-built and
   the dims are the allocation's — T5.E2's rule); `ppMbListD` and its context field
   (18 sites), `InitMbListD`'s two allocations and the `ppMbListD[0]` free are
   deleted; `InitMbInfo` fills the array through `from_vec` or in place.
   **`GetRefMb` reads a *different* layer's array** through the face-1 index —
   two distinct `Box`es, so no borrow conflict, and the resolution is one
   `ppDqLayerList` lookup.
3. **The three per-slice-index arrays** — `pFirstMbIdxOfSlice` (20 sites),
   `pCountMbNumInSlice` (17), and the `ppSliceInLayer` `Vec` from face 1 — become
   `Vec`s on the layer, allocated in `InitSliceInLayer` (`svc_encode_slice.rs:2465`)
   and grown in `ExtendLayerBuffer` (`:2668`) by `resize`, which deletes that
   function's three malloc/copy/free triples outright.
4. **`SSliceCtx.pOverallMbMap: *mut u16`** (`slice_multi_threading.rs:217`, 33
   sites) → `Vec<u16>` on the inline `sSliceEncCtx` — plan §4's "maps → `Vec<u16>`"
   — allocated in `InitSlicePEncCtx` (`svc_enc_slice_segment.rs:533`), freed by
   `Drop` instead of `UninitSlicePEncCtx`'s explicit free. The
   `WelsSetMemMultiplebytes_c` fills (`encoder_ext.rs:2797`,
   `svc_encode_slice.rs:2130`) become `fill` over a subslice.
5. **Where a raw pointer must still be handed out** — every `*mut SMB` caller
   (130 sites) survives this face untouched — derive from the container's **root**:
   `mb_data.as_mut_slice().as_mut_ptr().add(xy)`, never
   `mb_data.as_mut_slice()[xy..].as_mut_ptr()`. That is **S28 verbatim**: the
   narrowing index gives a correct address with provenance for the tail only, and
   the neighbour reads (`pCurMb.offset(-1)`, `.offset(-iMbStride)`) walk backwards
   out of it. Put the reason at the accessor and give it the Miri test S28
   mandates.

## 4. Face 3 — the slice banks own

`sSliceBufferInfo[i].pSliceBuffer: *mut SSlice` (20 sites) → `Vec<SSlice>`, now
that the layer is `Box`-built and `ppSliceInLayer` holds indices.

- `InitSliceThreadInfo`'s `WelsMallocz(size_of::<SSlice>() * n)`
  (`svc_encode_slice.rs:2443`) becomes `vec![SSlice::new(); n]` — and `SSlice`'s
  own `Default` is `mem::zeroed()` (`:307`), which must become a real constructor
  for the same reason as the layer's. Session C made `SSlice` 6544 bytes of mostly
  inline scratch, so state whether the constructor is written field-wise or built
  once and cloned, and **measure** that the first frame's output does not move.
- **`ReallocateSliceList` (`:2542`) becomes a `resize`, and a real defect closes
  with it** — the one session C handed over: it `copy_nonoverlapping`s the old
  slices (raw `sSliceBs.pBs` included) into the new block, and its three error
  paths then `FreeSliceBuffer` the *new* list, freeing the *old* list's `pBs`
  allocations while the old list is still live and still owns them. Under
  `Vec<SSlice>` with `pBs` owned there is no second owner to free. **Show the
  fix rather than assert it**: name the double-free path in the log and, if a test
  can reach it, write one.
- `InitOneSliceInThread` (`:2353`), `CheckAllSliceBuffer` (`:2748`),
  `ReOrderSliceInLayer` (`:2758`) and `wels_task_management.rs`'s task path index
  the banks instead of adding to a pointer. **Phase 7's boundary holds**: the task
  still takes `*mut SSlice` for the slice it claims — derived at the claim, from
  the bank root per §3.5 — and *claiming* is not rewritten here.

## 5. Face 4 — the span

Faces 2–4 move per-macroblock addressing on the mode-decision, motion-estimation,
entropy and deblocking paths, so the session owes a span (S2b, D-gate-1):
`085e2e41` → the close, both benches, interleaved pairs through `perfpair.py`, a
null re-run at the verdict's own pair count, a second day for any reading a
decision rests on. Ledger row with the D-perf-4 arithmetic stated (encoder
cumulative ≈ **+10…+12%** after session C, tripwire +25% median).

A reading outside the null band names its mechanism as a **candidate, not a
claim** (S33). The ones on the table: `MbArray`'s bounds checks where pointer
arithmetic was, and the `ppSliceInLayer` indirection becoming an index. Session C
predicted an AoS cost and measured **−0.97%** instead, so predict nothing here
either — measure, then attribute.

## 6. Drop-from-the-end — one place, named

If the session runs out of room, **the `*mut SMB` and `*mut SMbCache` parameter
families stop at face 4's close and go to session E with their counts**
(`*mut SMB` 130 sites over thirteen files; `pMbCache` per session C's tally).
That is a clean family boundary: faces 2–4 convert *storage*, the parameter
families convert *signatures*, and §3.5's root-derived accessor is exactly the
seam between them. Nothing else in this brief may be dropped, and a drop is
written with its reason (phase6.md §6).

## 7. Done-test, and the close

1. The dynamic-slice probe exists, is green, its name is in the `--lib` output,
   its slice-count assertion is **measured** non-vacuous, and whether it reaches
   `ReallocSliceBuffer` is stated with the number behind it. Its solo cost is
   recorded beside the other three.
2. `pRefLayer` and `ppSliceInLayer` are indices; the S34 grep is in the log; the
   MT re-stamp gap is recorded as a finding owned by Phase 7, not fixed here;
   `pFeatureSearchPreparation` and its dead function are deleted (S18).
3. `SDqLayer` is `Box`-built with a real constructor, owns its `MbArray<SMB>`,
   its three per-slice `Vec`s and its `Vec<u16>` MB map; `ppMbListD` and
   `sMbDataP` are gone; `ExtendLayerBuffer`'s malloc/copy/free triples are gone;
   every `assert_size!` that stopped being true was deleted or re-pinned **in the
   commit that moved it**.
4. The slice banks are `Vec<SSlice>`; `ReallocateSliceList` is a resize; the
   error-path double free is closed and named; `SSlice::default()` is no longer
   `mem::zeroed()`.
5. Every raw pointer still handed out of an owned container is derived from the
   **root** (S28), with the reason at the accessor and a Miri test that reads its
   full legal reach in both directions.
6. Both sweeps 341/341 in both profiles after each face; all four probes green at
   each face's close.
7. The span is measured and ledgered per §5 with the tripwire arithmetic.
8. `exit` battery; F3 per S14 at adjudication time (session C left the ledger at
   measurement 67, 21 alternations, 40 acquittals); log entry ≤ 40 lines;
   `phase6.md` §6 shows D spent and E next with anything dropped named; plan §0's
   row updated; ratchet's encoder-side numbers before/after **with the unit**.

## 8. Non-goals

**`pCurDqLayer` does not convert here** — 271 sites, and it is a *context* field:
the context is `mem::zeroed`-built until 6.6, so the same S21 argument that lets
the layer own forbids the context to, and session G owns that flip. The layer
being `Box`-built keeps every `pCurDqLayer` deref valid meanwhile; say so in the
log rather than re-deriving it later.

Also out: the plane families `pCsData`/`pEncData` (21 + 6 sites) and
`pRefPic`/`pDecPic`/`pRefOri` — session F's, with the `SPicture` settlement session
B wrote. MT claiming, `pThreadBsBuffer`, the task queue — Phase 7's. The ME/MD/RC/
CABAC records — E's. SIMD — E's. `api/` — Phase 8's. No perf work beyond the span
(D-perf-6). No decoder work. No golden movement.
