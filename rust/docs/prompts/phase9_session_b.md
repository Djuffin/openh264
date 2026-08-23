# Phase 9 — Session B: the plane-cursor family, part one

## What this project is, in one paragraph

This repository is a port of the openh264 video codec from C++ (under `codec/`) to
Rust (the crate at `rust/crates/openh264-rs/`). The Rust crate is a drop-in replacement
for `libopenh264`: same C API, same output bytes, same error codes. The port was first
written as a near-literal translation full of raw pointers; since then it has been made
safe piece by piece, always keeping the output byte-identical. The decoder is done — it
compiles under `#![deny(unsafe_code)]` with essentially no exceptions. The encoder is the
remaining work. Every encoder file already carries `#![deny(unsafe_code)]`; it compiles
only because each remaining unsafe item is marked with an `#[allow(unsafe_code)]` and a
comment tag saying which family of work will remove it. **Phase 9 removes those tags by
making the code actually safe.** You are session B of that phase.

## What session B is about

The encoder still reaches picture pixels through raw pointers. A "plane" is one of the
three pixel arrays of a picture (luma Y, chroma U, chroma V), stored row by row with a
stride and a padding border. Today the encoder carries `*mut u8` pointers into those
planes in several places, and every kernel that reads or writes pixels — SAD/SATD cost
functions, block copies, the DCT, intra prediction, motion compensation, deblocking — is
reached through a small `unsafe` wrapper ("shim") that turns a raw pointer back into a
slice and calls an already-safe kernel.

This session's job: **replace the raw plane pointers with the safe plane types the
codebase already has, in the parts of the encoder where that can be done now, and
produce the exact design for the part that cannot.** The family is big (the census
counts 64 tagged items in the encoder plus roughly 80 untagged unsafe sites in the
shared `common/` kernels), so it is split across two sessions. This brief is the first
half and says precisely where the line is.

Work the steps in order. Each has a Goal, the Facts you need, what to Do, and the
Accept bar. Commit as `T9.B1`, `T9.B2`, … . Findings go to `rust/docs/phase9_findings.md`
starting at **F104**. Leave progress notes in `rust/docs/safety_refactor_log.md` as you go.

---

## Rules that never bend

1. **No behaviour change. Every commit is byte-identical.** The decoded-picture goldens,
   the 2919-row malformed-stream corpus, both benchmark binaries, and the diffharness
   sweeps must produce exactly the same bytes and codes before and after each commit.
   If a byte moves, that is a defect in your change — find it by bisecting your own small
   commits. Never re-pin a golden.
2. **Use the safe types that exist; do not invent new ones.** (Described below.)
3. **The unsafe counts only go down.** Run `rust/tools/unsafe_ratchet.sh check` before
   every commit; it fails if any file's count of `unsafe fn` / `unsafe {` / `*mut` /
   `*const` / `#[allow(unsafe_code)]` went up. Each conversion deletes wrappers and their
   tags. If you genuinely must raise a count (rare), rebaseline with
   `unsafe_ratchet.sh generate` in the same commit and say why in the message.
4. **Gates.** Per commit: `rust/tools/gates.sh commit` (2–3 minutes: builds every target,
   runs `cargo test` in debug and release — which already includes the conformance
   streams and the malformed corpus — plus the ratchet and a duplicate-type census). At
   the session close: `rust/tools/gates.sh family`, which adds the diffharness sweeps in
   both build profiles (the better part of an hour) — this is your byte-parity proof for
   the session. **Do not run Miri, benchmarks, or any performance measurement.** Miri runs
   once at the end of the whole phase; performance work is a separate phase.
5. **New code is safe Rust, full stop.** If a raw pointer is forced on you by a caller you
   cannot change this session, keep the shim, keep its tag, and note it for the next
   session. Never add a new raw-pointer signature.
6. **Stay in your lane.** Do not touch: anything in `src/api/` (the frozen C boundary);
   items tagged `SCREEN_CONTENT(dormant)`; any function whose raw parameter is
   `*mut sWelsEncCtx` (the encoder context — converting those is blocked for a reason
   explained below); `*mut SMbCache` / `*mut SMB` parameters (the per-macroblock metadata
   family, a later session); `*mut SDqLayer` / `*mut SSlice` parameters (the layer family,
   later); the threading code in `slice_multi_threading.rs` except as step 1 directs.

---

## The background you need

### Why the conversion order matters (the thing that makes Phase 9 hard)

The encoder's main state is one big struct, `sWelsEncCtx` (65 fields). The port's
dominant idiom is: take a raw pointer to something *inside* the context (a plane, a
coefficient buffer, a slice), call a function that takes the context as `*mut sWelsEncCtx`,
then keep using the pointer. Under Rust's aliasing model that is fine **only while the
pointer is raw**: a raw write through the context invalidates just the bytes it touches.
The moment that function parameter becomes `&mut sWelsEncCtx`, the call re-tags the whole
context and invalidates every derived pointer the caller is holding — Miri reports UB.
This was measured: a previous session converted 109 such parameters to `&mut`, all tests
passed, and Miri rejected it; it was reverted. So the raw *cursors* (planes, coefficient
buffers, macroblock metadata, layer pointers) must be converted **first**, each into an
owned-or-borrowed safe form that is not a pointer into the context; only after that can
the `*mut sWelsEncCtx` parameters become borrows. That is why you are doing planes now
and must not touch the context parameters.

### The safe types you will use (already built, already proven)

In `src/safe/plane.rs`:
- `PaddedPlane` — owns one plane's bytes, knows width, height, stride, padding, and
  where the visible origin is. A picture owns three of them.
  - `plane.cursor(x, y) -> PlaneCursor<'_>` — a read cursor positioned at sample (x, y).
  - `plane.cursor_mut(x, y) -> PlaneCursorMut<'_>` — a write cursor.
  - `plane.row(y, x0, len)` / `row_mut(...)` — a slice of one row.
- `PlaneCursor<'a>` — a borrowed, bounds-checked read position: `at(dx, dy)`,
  `row(dy, dx0, len)`, `advance(dx, dy)`, `stride()`.
- `PlaneCursorMut<'a>` — the write version: `at`, `set(dx, dy, v)`, `row_mut`,
  `reborrow(dx, dy)`, `advance`.

In `src/encoder/picture.rs`:
- `SPicture { planes: [PaddedPlane; 3], ... }` with safe accessors
  `plane(i) -> &PaddedPlane`, `plane_mut(i) -> &mut PaddedPlane`,
  `planes_mut3() -> [&mut PaddedPlane; 3]`.
- Two picture pools: `SrcPicPool` (the source/spatial pictures, owned by the preprocessor
  as `m_pSpatialPicPool`) and `RecPicPool` (the reconstruction/reference pictures, owned
  by each layer's `SRefList` as `pRef`). Pictures are named by handles, `SrcPicId` and
  `RecPicId`, resolved with `pool.get(id)` / `get_mut(id)`, or through
  `SRefList::pic(id) -> &SPicture` / `pic_mut(id) -> &mut SPicture`.

The **decoder** already did exactly this conversion. Its per-macroblock code reaches
pixels as `plane_mut(i).cursor_mut(x, y)` with sample coordinates, not byte offsets; its
intra-prediction kernels are safe functions `fn(pred: &mut PlaneCursorMut<'_>)` stored in a
plain mode-indexed array `[Option<fn(&mut PlaneCursorMut<'_>)>; 14]` — no `unsafe`, no
`extern "C"`. When in doubt about shape, look at how `src/decoder/decode_slice.rs` and
`src/decoder/decoder_core.rs` (search `pGetI4x4LumaPredFunc`) do it and copy.

### The already-safe kernels under the shims

The pixel kernels are **already safe**. Examples (same file each):
- `src/common/sad_common.rs:468` — `pub fn sample_sad<const W, const H>(a: &PlaneCursor, b: &PlaneCursor) -> i32`
- `src/encoder/sample.rs:47` — `pub fn satd_4x4(c1: &PlaneCursor, c2: &PlaneCursor) -> i32`
- `src/common/copy_mb.rs:56` — `pub fn copy_4x4(src: &PlaneCursor, dst: &mut PlaneCursorMut)`
- `src/encoder/encode_mb_aux.rs:255` — `pub fn dct_4x4(dct: &mut [i16; 16], pix1: &PlaneCursor, pix2: &PlaneCursor)`
- `src/encoder/get_intra_predictor.rs:284` — `pub fn i4x4_luma_pred_h(pred: &mut [u8; 16], reference: &PlaneCursor)`

What is `unsafe` is the shim around each, shaped like:
```rust
pub unsafe extern "C" fn WelsSampleSatd4x4_c(pSample1: *mut u8, iStride1: i32,
                                             pSample2: *mut u8, iStride2: i32) -> i32 {
    // builds two PlaneCursors out of the raw pointers with from_raw_parts, or loops on
    // raw pointers directly, then computes what satd_4x4 computes
}
```
The shims exist because the callers hand in raw pointers, and because the **dispatch
tables** hold raw-signature function pointers. Delete a shim by fixing its callers and
re-typing its table slot; the kernel underneath stays.

### The three ways the encoder reaches a plane today (this is the map of the work)

**(1) Dispatch tables of raw function pointers.** `SWelsFuncPtrList` (in
`src/encoder/wels_func_ptr_def.rs`) and `SSampleDealingFunc` (in `src/encoder/md.rs:706`)
hold `Option<unsafe extern "C" fn(...)>` slots, filled once at init:
- `pfGetLumaI4x4Pred: [Option<PGetIntraPredFunc>; 14]`, `pfGetLumaI16x16Pred: [...; 7]`,
  `pfGetIChromaPred: [...; 7]` where
  `PGetIntraPredFunc = unsafe extern "C" fn(pPrediction: *mut u8, pRef: *mut u8, kiStride: i32)`
  (`wels_func_ptr_def.rs:50`). Filled in `get_intra_predictor.rs:1265–1300` (28 slots).
- `pfSampleSad / pfSampleSatd / pfSample4Sad: [Option<PSampleSadSatdCostFunc>; BLOCK_SIZE_ALL]`
  where `PSampleSadSatdCostFunc = unsafe extern "C" fn(*mut u8, i32, *mut u8, i32) -> i32`
  (`md.rs:254`). Filled in `sample.rs:332–362` (`WelsInitSampleSadFunc`, 21 slots). The SAD
  shims live in `src/common/sad_common.rs` (14 of them), the SATD shims in `sample.rs` (7).
- `pfCopy8x8Aligned`, `pfCopy16x16Aligned`, `pfCopy4x4`, … (`PCopyFunc =
  unsafe extern "C" fn(*mut u8, i32, *mut u8, i32)`, `encode_mb_aux.rs:202`) and
  `pfDctT4` / `pfDctFourT4` (`PDctFunc = unsafe extern "C" fn(*mut i16, *mut u8, i32, *mut u8, i32)`,
  `:203`). Filled in `encode_mb_aux.rs:975–1020` (`WelsInitEncodingFuncs`).
- The deblocking filter slots (`deblocking.rs:233–250`), motion-compensation slots, and
  expand-picture slots — **not this session** (see the line below).

**The decoder's model for the fix:** the slot type becomes a *safe* function pointer,
e.g. `Option<fn(pred: &mut [u8; 16], reference: &PlaneCursor<'_>)>`, and the array is filled
with the safe kernels directly (`Some(i4x4_luma_pred_h)`). Safe fn pointers are ordinary
safe values; the table stays a table, just of safe functions.

**(2) The per-macroblock cursor bundle `SPicData`.** Defined at
`src/encoder/encoder_context.rs:157`:
```rust
pub struct SPicData {
    pub pEncMb: [*mut u8; 3],   // the source picture, at this macroblock
    pub pDecMb: [*mut u8; 3],   // the reconstruction picture, at this macroblock
    pub pRefMb: [*mut u8; 3],   // the reference picture, at this macroblock
    pub pCsMb:  [*mut u8; 3],   // the reconstruction picture again (the "cs" = current-slice view)
}
```
It lives inside `SMbCache` (`md.rs:491`) and is **stamped per macroblock** by
`WelsMdIntraInit` (`src/encoder/svc_base_layer_md.rs:312–366`): at the start of a row it
computes `layer.pEncData[0].offset((mbX + mbY*stride) << 4)` for each plane, and for
every following macroblock it adds 16 (luma) / 8 (chroma) to each pointer. The layer's
roots `SDqLayer.pEncData / pCsData: [*mut u8; 3]` (`svc_encode_slice.rs:1014–1018`) were
themselves stamped once per frame in `encoder_ext.rs:2157–2170` from
`m_pSpatialPicPool.get_mut(idEnc).planes().pData[...]` and `pRefList.pic_mut(idDec).planes().pData[...]`.
Consumers then do `let pPred = (*pMbCache).SPicData.pCsMb[0];` and hand the pointer plus a
stride to a shim. Counts of `SPicData` uses: `svc_base_layer_md.rs` 73,
`svc_mode_decision.rs` 35, `svc_encode_slice.rs` 10, `svc_encode_mb.rs` 6.

**The intended replacement ("the carrier holds names, not pointers"):** `SPicData`
stores *which pictures* and *where*, not pointers:
```rust
pub struct SPicData {
    pub enc: Option<SrcPicId>,   // source picture
    pub dec: Option<RecPicId>,   // reconstruction picture
    pub refp: Option<RecPicId>,  // reference picture
    pub mb_x: i32, pub mb_y: i32,
}
```
and a consumer that needs the source pixels at this macroblock builds its cursor at the
point of use: `src_pool.get(enc).plane(0).cursor(16 * mb_x + dx, 16 * mb_y + dy)`. This is
exactly the move the decoder made (pictures by `PicId`, macroblock by coordinates).

**(3) Whole-picture raw roots via `planes()`.** `SPicture::planes(&mut self) -> PicPlanes`
(`picture.rs:358`) returns `PicPlanes { pData: [*mut u8; 3], iLineSize: [i32; 3], ... }`
— three raw root pointers obtained through `data_ptr(i)`, which reads the address out of the
owned plane in a way that is deliberately *retag-stable* (repeated calls give sibling
pointers, not a stack — a subtle requirement documented above `data_ptr`). There are 32
`planes()` call sites (`wels_preprocess.rs` 17, `encoder_ext.rs` 4, `ref_list_mgr_svc.rs` 4,
`deblocking.rs` 2, `svc_mode_decision.rs` 2, …) and 94 uses of `.pData[...]`
(`wels_preprocess.rs` 49, `encoder_context.rs` 16, `encoder_ext.rs` 12, `deblocking.rs` 6,
`svc_base_layer_md.rs` 6, `svc_mode_decision.rs` 4). Plus 44 calls to the `&mut`
picture accessors `pic_mut(id)` / `layer_dec_pic_mut(layer)` / `ctx_pic_ref_mut(ctx, r)`
(`ref_list_mgr_svc.rs` 17, `svc_mode_decision.rs` 6, `svc_encode_slice.rs` 6, …).

These roots feed (2): the layer roots and the per-MB bundle are *derived* from them. Once
(2) names pictures by handle, most of (3) stops being needed.

### The hard part, stated plainly: the multi-threaded reconstruction write

When the encoder runs with several threads, each thread encodes some of the frame's
slices and **writes its reconstructed macroblocks into the same reconstruction picture**.
The macroblocks are disjoint, but a slice can start and end in the middle of a macroblock
row, so two threads can both write into the *same row* of the plane at different columns.
That means the plane's byte buffer **cannot be split into one contiguous `&mut [u8]` per
thread**. Today this is sound only because the threads write through raw pointers
(disjoint raw writes are fine under the aliasing model). A previous session proved the
hazard the safe way would create: if each thread obtains the reconstruction picture
through `pic_mut(id) -> &mut SPicture` and then `planes()`, two threads re-tag the same
picture and Miri reports a data race — which is why the multi-thread Miri probe
`fork_join_encodes_a_multi_slice_frame_under_the_aliasing_checker`
(`src/encoder/svc_encode_slice.rs:4161`) is currently `#[cfg_attr(miri, ignore)]`.

So the plane family has two halves:
- **Safe now:** everything that reads the **source** picture or a **reference** picture
  (both are read-only while a frame is encoded — any number of shared borrows is fine
  across threads), and everything that works on **buffers the macroblock cache owns**
  (prediction scratch buffers, coefficient blocks). That is SAD/SATD against source or
  reference, block copies between owned buffers and read-only pictures, the DCT (reads
  source + prediction, writes coefficients), the *prediction-buffer side* of intra
  prediction.
- **Needs a design decision:** anything that **writes the reconstruction picture** or
  reads it while another thread may be writing it — the reconstruction side of encoding a
  macroblock, intra prediction's reference pixels (read from the reconstruction picture),
  deblocking, and the multi-thread fork itself.

**This session converts the first half and produces the decision for the second.** Do
not attempt to make the multi-threaded reconstruction write "safe" by handing each worker
a `&mut` of the whole plane — that is the exact UB Miri already caught.

---

## Steps

### Step 0 — A census of the plane family by *caller* (one short commit)
**Goal:** know, before converting anything, exactly which plane consumers are safe-now
and which are blocked on the reconstruction-write decision — so you do not scope by
signature and get surprised (that happened to the coefficient family last session).
**Do:** for each raw plane shim and each raw-signature table slot (intra-pred ×28,
SAD/SATD/4SAD ×21, copy ×8, DCT ×2, deblocking, MC, expand), list its call sites and what
each passes: source picture / reference picture / reconstruction picture / an owned
`SMbCache` buffer / a local array. Mark each call site **ST-only** (runs only outside the
thread fork) or **in-fork** (runs in a slice-encoding worker). Add this as a section to
`rust/docs/phase9_census.md` (extend `rust/tools/phase9_census.py` if it can produce it;
otherwise a hand table with the commands that made it).
**Accept:** a table every later step cites; the "safe now" vs "blocked" split is
explicit per call site, not per function.

### Step 1 — The carrier becomes names: `SPicData` holds handles and coordinates
**Goal:** `SPicData` carries `SrcPicId` / `RecPicId` handles and macroblock coordinates
instead of raw pointers, and every consumer resolves a cursor at the point of use — with
the raw fields kept alongside *temporarily* so the tree stays green and byte-identical at
every commit (the strangler pattern).
**Facts:** stamped in `WelsMdIntraInit` (`svc_base_layer_md.rs:312–366`) from the layer
roots; the layer roots are stamped in `encoder_ext.rs:2157–2170` from the two pools; the
handles already exist there (`idEnc: SrcPicId`, `idDec: RecPicId`; the reference picture is
`SDqLayer::pRefOri` / the `PicRef` enum). 124 `SPicData` uses across four files.
**Do:** (a) add the handle+coordinate fields beside the raw ones; stamp both in
`WelsMdIntraInit` (handles from the layer, coordinates from `pCurMb.iMbX/iMbY`). (b) For
each consumer that reads the **source** (`pEncMb`) or **reference** (`pRefMb`) pointer,
replace the read with a cursor built from the pool:
`pool.get(id).plane(i).cursor(16*mb_x + dx, 16*mb_y + dy)` (chroma: 8×). Reaching the
pools from inside these functions is the only structural change: pass the pool (or the
specific `&SPicture`) in as a parameter from the caller that already has the context,
rather than reaching through a raw context pointer. (c) **Leave `pDecMb` / `pCsMb`
(reconstruction) consumers on the raw fields** — that is the blocked half (step 5).
(d) Delete a raw field only when its last reader is gone.
**Accept:** every source/reference read in `svc_base_layer_md.rs`, `svc_mode_decision.rs`,
`svc_encode_mb.rs`, `svc_encode_slice.rs` goes through a `PlaneCursor` built from a
handle; `pEncMb`/`pRefMb` raw fields deleted; byte-identical; ratchet down.

### Step 2 — SAD / SATD / 4-SAD tables become safe function-pointer tables
**Goal:** `pfSampleSad`, `pfSampleSatd`, `pfSample4Sad` hold safe
`fn(&PlaneCursor, &PlaneCursor) -> i32` (and the four-neighbour variant's shape); the 14
`WelsSampleSad*_c` shims in `common/sad_common.rs` and the 7 `WelsSampleSatd*_c` shims in
`sample.rs` are deleted; `common/sad_common.rs` goes `#![deny(unsafe_code)]`.
**Facts:** the safe kernels are `sample_sad::<W,H>` / `sample_sad_four::<W,H>`
(`sad_common.rs:468/485`) and `satd_*` (`sample.rs:47–109`). Callers: 41 `pfSampleSad`,
25 `pfSampleSatd`, 11 `pfSample4Sad`, plus ~60 direct `WelsSampleSad*_c`/`Satd*_c` calls
(`svc_mode_decision.rs`, `svc_base_layer_md.rs`, `svc_motion_estimate.rs`,
`svc_encode_mb.rs`). Each operand is a source/reference picture cursor (safe after step 1)
or an `SMbCache` prediction buffer (owned — build a `PlaneCursor` over the buffer slice
with its stride, e.g. `PlaneCursor::new(&cache.sMemPredMb[off..], 0, 16)`). **Check the
census:** any SAD/SATD operand that is the *reconstruction* picture stays on its shim,
tagged, for step 5.
**Do:** re-type the slot, change `WelsInitSampleSadFunc` to install the safe kernels,
convert the callers, delete the shims and their tags, add `#![deny(unsafe_code)]` to
`sad_common.rs` when its last `unsafe` is gone.
**Accept:** shims gone; `sad_common.rs` denies; callers pass cursors; byte-identical.

### Step 3 — Copy and DCT tables, and the intra-prediction *prediction* side
**Goal:** `PCopyFunc` slots become `fn(&PlaneCursor, &mut PlaneCursorMut)` and the
`WelsCopy*_c` shims go (`common/copy_mb.rs` goes deny); `PDctFunc` slots become
`fn(&mut [i16; N], &PlaneCursor, &PlaneCursor)` and `WelsDctT4_c` / `WelsDctFourT4_c` go;
the intra-pred tables are re-typed to `fn(pred: &mut [u8; N], reference: &PlaneCursor)`
**only if** step 0 shows the reference operand can be supplied safely at every call site
(it is the reconstruction picture — so in practice this lands in step 5; here, convert the
*prediction-buffer* plumbing: callers take `&mut [u8; 16/64/256]` from the owned
`sMemPredBlk4` / `sMemPredMb` halves instead of `mem_pred_blk4(pMbCache)` raw accessors).
**Facts:** DCT operands are source picture (safe) + prediction buffer (owned) → safe now.
Copies are between owned buffers and source/reconstruction — check each.
**Accept:** every table slot whose operands are all safe-now holds a safe fn; the
corresponding shims deleted; `copy_mb.rs` denies if its last raw caller is gone;
byte-identical.

### Step 4 — `common/` files that lost their last raw caller go `#![deny(unsafe_code)]`
**Goal:** lock in what steps 2–3 freed.
**Facts:** `common/` is the one tree with no `deny` at all; its unsafe is shims. Files:
`sad_common.rs` (21 sites), `copy_mb.rs` (3), `intra_pred_common.rs` (4 — the two
I16x16 V/H shims; reconstruction-side, probably step 5), `mc.rs` (36 — motion
compensation, next session), `deblocking_common.rs` (16 — next session), `expand_pic.rs`
(3 — next session), `wels_common_defs.rs` (3), `wels_trace.rs` (3), `cpu_core.rs` (0 —
can be denied today).
**Do:** add `#![deny(unsafe_code)]` to each file whose unsafe count reached zero; for any
that still has a site, tag the site with the family that owns it.
**Accept:** at least `cpu_core.rs`, `sad_common.rs`, `copy_mb.rs` under deny.

### Step 5 — The reconstruction-write design: measure, decide, write it up (do not implement blind)
**Goal:** the next session can convert the reconstruction-picture consumers and the
multi-thread fork against a decision that was measured, not guessed.
**Facts to establish by reading the code (record each with its anchor):**
- For each slice mode the encoder supports (`SM_SINGLE_SLICE`, `SM_FIXEDSLCNUM_SLICE`,
  `SM_RASTER_SLICE`, `SM_SIZELIMITED_SLICE`): can a slice boundary fall inside a
  macroblock row? (Read the slice partitioning code — search `WelsInitSliceMbBounds`,
  `iFirstMbInSlice`, `iCountMbNumInSlice` in `svc_encode_slice.rs` /
  `svc_enc_slice_segment.rs`.) If *every* multi-threaded mode is row-aligned, the plane
  can be split into per-thread contiguous row ranges (`split_at_mut`) and each worker gets
  its own `PlaneCursorMut` — the simple answer. If any mode is not, it cannot.
- What does a worker **read** from the reconstruction picture? Intra prediction reads the
  left/top neighbours *within the same slice* (H.264 does not predict across slice
  boundaries), so a worker reads only what it wrote. Confirm in the port (search the
  availability flags around the intra-pred call sites). Deblocking runs after the join.
- The fork today: `slice_multi_threading.rs` hands workers a `SliceJobHandle` carrying raw
  pointers under one `unsafe impl Send` (the one tagged `send-seam`); the job's plane
  access is through the raw layer roots.
**Decide and write (as a short design note in `phase9_findings.md` and a pointer in
`rust/docs/prompts/phase9.md` §4):** the recommended shape for the next session, one of:
  (a) row-aligned split — each worker gets disjoint `&mut` row ranges of each
  reconstruction plane, built once before the fork; consumers take `PlaneCursorMut`;
  (b) a per-worker **raw-backed** cursor type constructed at the fork seam (one tagged
  `unsafe` site per job per plane, justified by macroblock-disjointness), whose methods are
  safe and whose constructor carries the contract — consumers' code is safe, the seam is
  one place; the single-threaded path builds ordinary `PlaneCursorMut`s from `plane_mut`;
  (c) something else the reading justifies.
State which consumers (by file and count) wait on it, and the acceptance test: the Miri
MT probe at `svc_encode_slice.rs:4161` loses its `ignore` and passes.
**Accept:** the design note exists, with the slice-mode measurement, the read/write
analysis, the recommendation, and the list of blocked consumers. If the measurement shows
option (a) is available, say so loudly — it changes the next session's size.

### Step 6 — If short, drop from the end
Drop order: step 5's write-up can shrink to the measurement plus a one-paragraph
recommendation (never drop the measurement — it is the next session's first fact); then
step 4's extra `deny`s; then step 3's intra-pred plumbing (keep the DCT/copy); then the
tail of step 2's direct-call conversions (keep the table re-type and the installer).
Never drop step 0 or step 1's source/reference conversion — that is the session's spine.

### Step 7 — Close
`rust/tools/gates.sh family` in both profiles; `unsafe_ratchet.sh report`; the tag count
before/after (`grep -rhn 'unsafe-cat:' rust/crates/openh264-rs/src | sed 's/.*unsafe-cat: //;s/ *$//' | sort | uniq -c`);
the log entry; update the session table in `rust/docs/prompts/phase9.md` §8 with what
landed and what the next session inherits. Report.

---

## What to report back

1. Commits — hash and one line each.
2. The **step-0 census** (the caller table) — this and step 5's note are what the next
   session reads first.
3. Per step: what converted, which shims/tables/fields were deleted, the tag-count and
   ratchet deltas, and any call site left on a shim with its reason.
4. **Step 5's design note** — the slice-mode measurement (row-aligned or not, per mode,
   with anchors), the read/write analysis, your recommendation, and the blocked-consumer
   list with counts.
5. Byte parity: the `gates.sh family` verdict in both profiles; zero moved bytes.
6. Findings F104+, and any statement in this brief that the tree contradicted (quote both).
7. What the next session (the second half of the plane family) inherits, sized by the
   caller census.
