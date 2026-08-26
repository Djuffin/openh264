# Phase 9 findings

*Numbering continues Phase 8b's (which closed at F100). Each entry is a fact with the
grep or the run that produced it.*

## F101 — the charter's family table is a first-line artefact; "body-only" is 147, not 419

The Phase 9 charter (`prompts/phase9.md` §2) classifies the 695 `port-raw(Phase 9)`
tags into seven families and reports **419 body-only** — sites whose *signature* is
already safe, unsafe only because the body derefs a cursor, and which therefore
"fall out as their callees go safe — **not converted directly**". That is 60% of the
phase's queue declared free, and it is the load-bearing assumption behind the
charter's 10–14 session estimate.

It is wrong, and the way it is wrong is mechanical. The table was produced by reading
only the **first line** of each tagged item. The port wraps any signature past ~100
columns, so for every multi-line signature the first line is bare:

```rust
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsDctMb(          // <- all a first-line scan sees
    pRes: *mut i16,               //    the raw parameters are here
    pEncMb: *mut u8,
    iEncStride: i32,
    pBestPred: *mut u8,
    pfDctFourT4: Option<PDctFunc>,
) {
```

Restricting `phase9_census.py` to the first line reproduces the charter's table almost
exactly, which is what identifies it as an artefact rather than a measurement:

| family | charter | first-line scan | **whole signature** |
|---|---:|---:|---:|
| body-only | 419 | 420 | **147** |
| ctx param | 120 | 121 | **234** (111 ctx-only) |
| plane `*mut u8` | 51 | 51 | **64** (57 pure) |
| layer/pic/slice | 33 | 36 | **55** (47 pure) |
| other raw sig | 31 | 39 | **123** (80 pure) |
| coeff `*mut i16` | 25 | 19 | **19** (all pure) |
| SMbCache/SMB | 15 | 9 | **48** (45 pure) |
| dispatch | — | — | **5** |
| sum | 694 | 695 | **695** |

(The charter's own column sums to 694, not 695 — a second, smaller arithmetic slip.)

**272 sites move out of "body-only"** and into a family that has to convert their
signature. Two consequences the charter does not budget for:

1. **`other` is 123 sites and has no owner.** The charter's §8 session table assigns
   sessions to coeff, plane, mbcache, layer, dispatch and ctx; `other` (31 in the
   charter, 123 in fact) is named "case by case" and given to nobody. It is not one
   family — bitstream writers, parameter sets, the rate controller, LTR, VAA, deblock,
   trace and scalar out-parameters — and 80 of its sites are pure, i.e. independent of
   the cursor spine and runnable in parallel with B–E.
2. **`ctx` is 234, not 120.** 111 are ctx-only (F66's 109 convertible plus the two
   `*mut *mut sWelsEncCtx` out-parameters); the other 123 carry a cursor *as well as*
   the context and so convert only after that cursor's own family lands.

Verified by `rust/tools/phase9_census.py`, which reads the whole parameter list and
prints both the family table and the pointee-type inventory behind it. The one number
the charter got exactly right is `plane` at 51 — because `common/`'s plane shims
happen to have short, single-line signatures.

**S24 restated:** re-grep before quoting, and when a census disagrees with a
prose table, the tool that can be re-run wins.

## F102 — `q1c.py` was never committed and is not in the session-J log; rebuilt from spec

F66 (`phase6_findings.md`) and the charter (`prompts/phase9.md` §6) both instruct
Phase 9 to run session J's aliasing-hazard detector as the precondition to the
`*mut sWelsEncCtx` conversion, and both locate it the same way: "`q1c.py`, reproduced
in the session J log". It is not there.

```bash
grep -rn 'q1c' rust/docs/safety_refactor_log.md          # empty
git log --all --oneline -- '*q1c*'                       # empty
```

The only occurrences of the string in the tree are the four forward references that
promise it. The tool was written, used to produce F66's measurement, and lost with
session J's working directory.

Rebuilt from F66's written specification as `rust/tools/q1c.py` (T9.A2). Because it is
a reimplementation, its counts are not expected to match session J's — and session J's
own are not self-consistent: F66 reports "93 sites in 28 callers" directly above a
per-file table (`encoder_ext.rs` 81, `ref_list_mgr_svc.rs` 32, `rc.rs` 31, and four
more) that sums to **165**. Neither figure can be reproduced against the other, so
both are treated as measurements of the same shape rather than as a target.

The rebuild is validated the only way that is meaningful: **against the two sites Miri
actually confirmed**, which F66 quotes in full. Both are found, and they turn out to
be *different shapes* —

* **Shape A, a cursor held across the call** — `pSpatialIndexMap` derived from the
  context, then `InitBitStream(pCtx)`, then the cursor read
  (`encoder_ext.rs:3256–3260` today; F66 quotes it at `:3033`). The remedy is to
  retire the cursor's family first; reordering is not available in general because
  several of these callees reallocate the container the cursor points into.
* **Shape B, argument evaluation order** — `WelsRcInitModule(pCtx, (*ctx_param(pCtx)).iRCMode)`
  (`encoder_ext.rs:1524`). Arguments evaluate left to right, so once parameter 1 is
  `&mut sWelsEncCtx` it takes the Unique retag and argument 2 then reads through a raw
  that is already dead. The remedy is local — hoist the argument — and session J did
  exactly that.

A triple-pattern detector alone finds only shape A, so a rebuild that had stopped
there would have been green on one of the two sites Miri had already caught. Shape B
is 62 of the 287 sites now reported and is worth having as its own category, because
its fix is a hoist rather than a family conversion.

One thing the rebuild forced that the specification does not mention: the context root
is **not always a parameter**. `WelsInitEncoderExt` already takes an owned
`&mut Option<Box<sWelsEncCtx>>` and makes the raw context a *local*
(`let pCtx = Box::into_raw(...)`, `encoder_ext.rs:1478`) — and that function holds
shape B's confirmed site. A parameter-only root scan sees no context there at all.
The detector therefore propagates local roots as well: explicit `*mut sWelsEncCtx`
types and casts, `Box::into_raw`, a deref of a `*mut *mut sWelsEncCtx` parameter, and
plain aliases.

Current reading at `aabd0da5`: **287 hazardous sites in 68 callers across 84 distinct
ctx-taking callees** (225 shape A over 42 distinct cursors, 62 shape B); 184 of the
268 ctx-taking callees have no detected hazard. Per F66 a clean callee means "no
hazard found", **not** "proved safe" — F66 rejected converting the clean subset for
precisely that reason, and this tool's docstring repeats the warning.

## F103 — the coefficient family is 5 kernels, not 25: the other 14 are gated on SMbCache

Session A's brief scopes the coefficient conversion as "the `WelsQuant*`/`WelsScan*`/
`WelsCalculateSingleCtr`/`WelsGetNoneZeroCount` shims", on the premise that
"every input to the coefficient-only kernels is already an owned `[i16; N]` on the
caller side; the raw pointer is a `as_mut_ptr()`/`from_raw_parts` round-trip that
deletes cleanly". That premise holds for four kernels and fails for fifteen.

Of the 19 coeff-pure signatures the census finds:

| | kernels | why |
|---|---|---|
| **directly called** | 5 | caller passes an owned `[i16; N]`; converts cleanly |
| **`SWelsFuncPtrList` slot** | 14 | no direct caller at all outside their own unit tests |

```bash
grep -rn 'Some(WelsQuant4x4_c)\|WelsQuant4x4_c *(' rust/crates/openh264-rs/src
#   encode_mb_aux.rs:1009  f.pfQuantization4x4 = Some(WelsQuant4x4_c);   <- the only
#   encode_mb_aux.rs:1055  (a unit test)                                    real ref
```

The fourteen are reached only through the table, and **the 59 call-through sites do
not pass owned arrays**. They pass SMbCache-derived walking cursors
(`svc_encode_mb.rs:511`, in `WelsEncRecI16x16Y`):

```rust
let mut pRes = crate::encoder::md::coeff_level(pMbCache);
// S28: derived from the whole array, not `[0].as_mut_ptr()` — this cursor walks
// every 4x4 block, and a tag narrowed to one block dies at the second.
let mut pBlock = std::ptr::addr_of_mut!((*md::dct(pMbCache)).iLumaBlock).cast::<i16>();
...
for _ in 0..4 {
    if let Some(func) = (*pFuncList).pfQuantizationFour4x4 { func(pRes, pFF, pMF); }
    if let Some(func) = (*pFuncList).pfScan4x4Ac {
        func(pBlock, pRes);
        func(pBlock.add(16), pRes.add(16));      // <- the walk
        ...
    }
    pRes = pRes.add(64);
    pBlock = pBlock.add(64);
}
pRes = pRes.sub(256);
```

So converting the fourteen coefficient slot types to `&mut [i16; N]` requires the
`SMbCache` cursors to be owned first, and requires the walking cursor to become a
`chunks_mut` walk. **That is family 3 (session D), not family 5.** The brief's
caution — "if it is still dispatched through a table this session does not retire,
convert the direct calls and leave a thin `unsafe` shim for the table slot, tagged
as family 5" — points at the wrong owner: family 5 is the *five self-referential*
slot types (`PDeblockingBSCalc`, `PMotionSearchFunc`, …) that de-virtualize
independently. The coefficient slots are ordinary dispatch and follow their data.

The general lesson, now written into `phase9_census.py`: **`pure` is a property of
the signature, not of the call sites.** A single-family signature is convertible
only if its callers already hold the safe type. Sessions B–E should grep their call
sites before scoping, not just the signatures — the same gap will appear in the
plane family, where the shims are called with picture-pool cursors.

Two smaller facts found the same way:

* **`WelsScan4x4Dc` has no production caller.** Its only reference in the whole
  workspace is `tests/kernels_differential_phase2.rs:1500`, which asserts the shim
  agrees with `scan_4x4_dc_ac` — the kernel it forwards to. It is not in any
  dispatch slot. (`src/`-only greps say it is dead; it is not, and that is why the
  grep has to cover `tests/` too.) Converted here; the test it serves becomes
  tautological and the pair is an S18 deletion candidate for a later session.
* **`cargo build --all-targets` earned its place again.** Converting the three
  `svc_encode_mb.rs` kernels compiled clean as a library and broke three call sites
  in `kernels_differential_phase2.rs`. `cargo test` alone builds integration tests,
  so it would have caught these — but the same change class reaches benches, which
  it does not build. The gate's own header documents that trap from Phase 5.

## F104 — the plane family's cost tables are gated on **two** families, not one; the caller census says so

Session B's step 0 built the plane family's **caller** census
(`rust/tools/phase9_plane_callers.py`, `rust/docs/phase9_plane_census.md`): every
call site of a raw plane entry point, with each pointer operand classified by the
surface it names. 109 call sites (plus 64 kernel-internal composition calls that
die with their shims). The verdict distribution is the session's scope:

| verdict | sites | meaning |
|---|---:|---|
| `safe-now` | 64 | every plane operand is source / reference / owned scratch |
| `blocked` | 32 | an operand is the **reconstruction** picture (F107) |
| `coeff` | 13 | an operand is a coefficient block — family 3's (F103) |

The headline is not the distribution, it is the **pairing**:

* **Every one of the 37 cost-table call sites** (`pfSampleSad`, `pfSampleSatd`,
  `pfSample4Sad`, and the `md_cost`/`me_cost` selectors) is `safe-now` on its
  plane operands — and **25 of the 37 pair a source/reference operand with an
  `SMbCache` prediction buffer** reached through `md::mem_pred_*`. A table has one
  type, so re-typing the three slots to safe function pointers is atomic across
  all 37 sites, and it cannot happen until *both* the plane roots (source and
  reference, through the pools) *and* the `SMbCache` buffers (family 3, session D)
  are safe. Twelve sites are source x reference only
  (`svc_motion_estimate.rs` x10, `CalUVSadCost`, `scene_change_detection.rs:87`)
  and would convert on the plane roots alone; they cannot go first because the slot
  type is shared with the other 25.
* The same double gate holds for **`mc`** (23 sites, all reference x cache) and for
  **`dct`** (13 sites, all coefficient).

Two consequences the brief and the charter do not carry:

1. **`common/sad_common.rs`, `common/mc.rs` and `encoder/sample.rs` cannot reach
   `#![deny(unsafe_code)]` before family 3.** The charter's §4 has `common/` going
   `deny` "as the encoder callers that reach their shims convert", and session B's
   brief lists `sad_common.rs` under `deny` as an acceptance criterion. Neither is
   available while the second operand of every cost call is an SMbCache buffer.
2. **The DCT table is coefficient-gated, all 13 sites.** Session B's brief scopes it
   as "DCT operands are source picture (safe) + prediction buffer (owned) -> safe
   now"; the `pDct` parameter is neither. It is `md::coeff_level(pMbCache)` or
   `md::dct(pMbCache)` at every call site — the SMbCache walking cursor F103
   describes, in the same file and the same loop shape:

   ```rust
   // svc_encode_mb.rs:494-511, WelsEncRecI16x16Y
   let mut pRes = crate::encoder::md::coeff_level(pMbCache);
   ...
   WelsDctMb(pRes, (*pMbCache).SPicData.pEncMb[0], kiEncStride, pBestPred,
             (*pFuncList).pfDctFourT4);
   ```

   `PDctFunc` therefore follows its coefficient operand into family 3, exactly as
   the fourteen `PQuant*`/`PScan*` slots did.

**What is left for the plane family alone**: the intra-prediction table (5 sites,
all reconstruction-gated), the inverse-DCT table (10 sites, reconstruction
destination *and* a coefficient operand) and 17 of the 21 copy sites — i.e. the
*whole* remainder is F107's, and it is session C's.
The plane family, read from the callers, is not "convert the plane roots and the
shims fall out". It is: **plane roots first (they unblock nothing on their own),
then family 3, then the tables flip in one commit each.**

Method note, and it is the same one F103 made from the other side: a signature
census answers "what could this family convert if nothing else were in the way",
and the answer to the question a session actually needs — "what can I convert
now" — is only in the callers. Both tools now exist and
`phase9_census.py`'s header points at the second.

## F105 — 20 of `common/mc.rs`'s 29 raw kernels have no production caller

`mc_luma`'s `match` (`mc.rs:733-760`) replaced the module-internal
`pWelsMcFunc_c: [[fn; 4]; 4]` table in Phase 2, and `mc_chroma` did the same for
its own. The per-fractional-position raw shims the table used to hold were kept as
"strangler shims ... the raw-pointer entry points `SMcFunc` still holds"
(`mc.rs:1079`) — but `SMcFunc` holds only three slots (`pMcLumaFunc`,
`pMcChromaFunc`, `pfSampleAveraging`, `InitMcFunc` at `:2219`), and the twenty
below have no reference in `src/` at all except their own definitions:

```bash
grep -rn '\bMcHorVer13_c\b' rust/crates/openh264-rs/src
#   src/common/mc.rs:1767:pub unsafe extern "C" fn McHorVer13_c(     <- the definition
```

The twenty: `McCopy_c`, `McCopyWidthEq2/4/8/16_c`, `McHorVer01/03/10/11/12/13/21/23/30/31/32/33_c`,
`McChromaWithFragMv_c`, `FilterInput8bitWithStride_c`, `HorFilterInput16bit_c`.
Their only other references are `tests/kernels_differential_phase2.rs` and
`benches/`. Live are `McLuma_c`, `McChroma_c`, `PixelAvg_c` (the three `SMcFunc`
slots) and `McHorVer02_c`/`McHorVer20_c`/`McHorVer22_c` (called directly by
`md.rs`'s `MeRefineFracPixel`).

This is an S18 deletion of **20 `unsafe fn`s of the 29 in `mc.rs`**, and it
resizes session C: the charter counts `mc.rs` at 36 unsafe sites and treats them
as conversions. Most are deletions. The differential tests that name them go with
them — they compare each shim against the safe kernel the shim itself calls, so
they are tautological in the F103 sense, and the live path (`mc_luma`/`mc_chroma`
through the three slots) keeps its own coverage.

Not acted on here: `mc.rs` is session C's file and the deletion should land with
the conversion of its three live slots, not ahead of it.

## F106 — `common/expand_pic.rs`'s three raw items were dead, and their differential test was tautological

The census (F104) turned up no reference in `src/` to either border-expansion
shim:

```bash
grep -rn '\bExpandPictureLuma_c\b' rust/crates/openh264-rs/src
#   src/common/expand_pic.rs:153:pub unsafe extern "C" fn ExpandPictureLuma_c(   <- the definition
```

Both codecs' pictures own their planes since T5.AC5 / T6.F4, and each
`SPicture::expand_as_reference` hands `expand_picture` the plane's own
allocation. `ExpandReferencingPicture` — the dispatcher — was deleted then; the
two `_c` kernels and `expand_shim_span` were kept with the reason "they are the
C-shaped subjects `tests/kernels_differential_phase2.rs` runs against the
reference" (`expand_pic.rs:187-195`).

They were not run against the reference. The probe's golden was
`exp::expand_picture` — the function each shim calls two lines in — so the
equivalence it asserted was between a function and its own callee. What the probe
*did* still test was `expand_shim_span`'s backwards walk, which is the code being
deleted, plus two properties that belong to the kernel:

* every padding byte is written and none is read;
* slack columns of a wider-than-minimal stride stay untouched.

T9.B2 deletes all three items, moves those two properties onto `expand_picture`
itself (`expand_picture_writes_every_padding_byte_and_reads_none`), and puts
`common/expand_pic.rs` under `#![deny(unsafe_code)]`. `common/cpu_core.rs`, which
has no `unsafe` at all, goes under `deny` in the same commit. Byte-identical: the
deleted code had no caller.

Two files of `common/`'s nine are now denied. **The other seven are further away
than the charter reads**: `sad_common.rs`, `mc.rs` and `intra_pred_common.rs`
wait on family 3 and on the reconstruction write (F104), `deblocking_common.rs`
on the reconstruction write, and `wels_common_defs.rs` / `wels_trace.rs` are the
`other` family's, not this one's. `common/mod.rs` cannot carry the attribute at
all — an inner attribute there applies to every submodule.

Note the shape, because it recurs: F103 found it in `WelsScan4x4Dc`, F105 in
`mc.rs`'s twenty, and this is the third. **A shim kept alive only by a
differential test whose golden is the shim's own callee is dead code with a
green test on top of it.** The tests that survive a strangler conversion are the
ones whose golden is the *reference implementation*; when the reference side is
retired, the surviving assertions have to be re-pointed at the safe kernel or
they mean nothing.

## F107 — the reconstruction write: the measurement, and why the write view cannot be `&mut [u8]`

Session B's step 5. The question the plane family's second half turns on: the
encoder's workers all write into **one** reconstruction picture, so what shape can
that write take in safe Rust? Everything below is read out of the tree, with the
anchor for each fact.

### 1. Is any multi-threaded slice mode row-aligned?

A slice is a run of consecutive macroblock indices. If the run is a whole number
of macroblock **rows** and starts on a row boundary, the bytes it writes are a
contiguous band of the plane and `split_at_mut` can hand one band to each worker.
Otherwise two workers write into the same row of the plane at different columns,
and no split of one `&mut [u8]` expresses that.

Two multi-threaded entry points exist, both in `WelsEncoderEncodeExt`:

* `uiSliceMode != SM_SIZELIMITED_SLICE && iMultipleThreadIdc > 1` ->
  `EncodeFixedSlicesForked`, one job per **slice** (`encoder_ext.rs:3571-3606`);
* `uiSliceMode == SM_SIZELIMITED_SLICE && iMultipleThreadIdc > 1` ->
  `EncodeDynamicSlicesForked`, one job per **partition**
  (`encoder_ext.rs:3618-3660`).

| mode | forks? | run lengths come from | row-aligned? |
|---|---|---|---|
| `SM_SINGLE_SLICE` | no — `iSliceCount <= 1` returns `ENC_RETURN_UNEXPECTED` (`encoder_ext.rs:3580`) | whole frame | trivially |
| `SM_FIXEDSLCNUM_SLICE`, RC **on** (the default, and every diffharness row) | yes | `GomValidCheckSliceMbNum` (`svc_enc_slice_segment.rs:243-301`) | **YES** |
| `SM_FIXEDSLCNUM_SLICE`, `RC_OFF_MODE` | yes | `CheckFixedSliceNumMultiSliceSetting` (`:91-118`) | **no** |
| `SM_RASTER_SLICE` | yes | the caller's `uiSliceMbNum[]` (`:150-193`, `AssignMbMapMultipleSlices:415-480`) | **no** |
| `SM_SIZELIMITED_SLICE` | yes | `UpdateSlicepEncCtxWithPartition` (`encoder_ext.rs:2881-2934`) | **no** |

The one `YES` is worth its own paragraph, because it is not obvious.
`GomValidCheckSliceMbNum` assigns every slice but the last a multiple of
`iGomSize`, and `iGomSize = kiMbWidth * GOM_ROW_MODE0_*P` where the four
constants are **2 or 4** (`rc.rs:146-149`) — a GOM is two or four *macroblock
rows*. Every branch of the assignment stays on that grid (`iNumMbAssigning` is
`WELS_DIV_ROUND(..) * iGomSize`, the floor is `iMinimalMbNum = iGomSize`, the cap
is `(iMaximalMbNum / iGomSize) * iGomSize`), and the last slice takes
`kiMbNumInFrame` minus a sum of multiples of `kiMbWidth`, which is again a
multiple of `kiMbWidth`. The runtime re-slicer keeps the property:
`DynamicAdjustSlicing` rounds to `iNumberMbGom = iMbWidth * iGomRowMode0`
(`rc.rs:920`) in every RC mode, and its clamps are `kiCountNumMb` minus multiples
of `iMbWidth` (`slice_multi_threading.rs:617-635`). In `RC_OFF_MODE` it rounds to
nothing and `iMinimalMbNum` falls to `iMbWidth` or to **1**.

**So option (a) — "split the plane into per-worker row bands before the fork" —
is available for one of the four multi-threaded configurations and not for the
other three.** It cannot be the mechanism. It could be a fast path, but a second
code path for the same output is a divergence risk with nothing to gain.

### 2. What does a worker read of the reconstruction picture?

Only what it wrote itself, and this is enforced rather than incidental:

* **Intra prediction's reference pixels.** `UpdateMbNeighbor` sets `LEFT_MB_POS` /
  `TOP_MB_POS` / `TOPLEFT` / `TOPRIGHT` in `SMB::uiNeighborAvail` only when the
  neighbour's `uiSliceIdc` equals the current macroblock's
  (`svc_encode_slice.rs:1343-1346`). `FillNeighborCacheIntra` turns that into
  `SMbCache::uiNeighborIntra` (`md.rs:855-944`), which indexes the availability
  tables that choose the DC-128 / DC-left / DC-top variants — so an unavailable
  side is never read.
* **Deblocking.** `bLeftBsValid`/`bTopBsValid` are indexed by
  `SDeblockingFilter::uiFilterIdc`, and index 1 is
  `uiSliceIdc == neighbour's uiSliceIdc` (`deblocking.rs:903-913`). Under
  multi-threading `InitSliceSettings` forces `iLoopFilterDisableIdc = 2`
  (`encoder_ext.rs:1366`), which is `uiFilterIdc = 1`.
* **Motion compensation reads the *reference* picture, which is a different
  slot.** `PrefetchNextBuffer` picks `pDecPic` from the slots with
  `bUsedAsRef == false`, and `SetUnref()`s the one it takes in the fallback arm
  (`ref_list_mgr_svc.rs:660-691`); `WelsBuildRefList` admits only pictures with
  `bUsedAsRef == true` (`:1112-1121`). It runs from `EndofUpdateRefList` at the
  *end* of the previous frame (`:1756`), so `pDecPic ∉ pRefList0` by construction.
  (Measured on the `TemporalLayer` strategy — the camera path. The two `Screen`
  strategies are `SCREEN_CONTENT(dormant)`, Phase 10's.)

So **every byte of the reconstruction plane a worker touches belongs to one of its
own macroblocks**, and the byte sets of two workers are disjoint. That is the
premise any design has to discharge, and it is now measured.

### 3. Why `&mut [u8]` cannot express it — and this is the finding

Disjointness is not enough. The plane is one allocation, and under Stacked
Borrows a `&mut [u8]` is a `Unique` retag over its **whole range**, whether or not
the writes touch all of it. Three shapes all fail for that one reason:

* **`&mut SPicture` per worker** — what the port does today through
  `layer_dec_pic_mut` -> `SRefList::pic_mut` and `SPicture::planes`' `&mut self`.
  That is F73, and it is why the fork/join Miri probe carries
  `#[cfg_attr(miri, ignore)]` (`svc_encode_slice.rs:4160`).
* **A per-macroblock `PlaneCursorMut`.** `PlaneCursorMut::new(buf, center, stride)`
  takes a sub-slice, so the obvious narrowing is "the contiguous span from the
  macroblock's first row to its last". **That span is the full width of those
  rows**, because rows are contiguous in the buffer — so two workers on the same
  macroblock row hold overlapping `&mut [u8]`, and the second retag pops the
  first. Narrowing does not help; it is the same bug at a smaller radius.
* **`split_at_mut` bands** — sound, but only where the slices are row-aligned,
  which §1 measured at one mode of four.

The write view therefore **must not hand out `&mut [u8]` over anything wider than
one macroblock's columns**. Two shapes do satisfy that:

**(A) A shared, interior-mutable plane view — the recommendation.** One seam type
built once per plane on the calling thread from `&mut PaddedPlane` *before* the
fork (so the plane is exclusively borrowed for the fork's whole scope, which is
what removes F73), holding the buffer in an `UnsafeCell` and carrying one tagged
`unsafe impl Sync`. Workers share `&SharedPlane` and write through `&self`:
`set(x, y, v)`, `write_row(x, y, &[u8])`, `copy_row_from(..)`. Reads keep working
(`at`, `row`) because a shared view can read. The properties this buys:

* the `unsafe` is **one `impl`, in one file**, and its contract is exactly what §2
  measured — no two live write views overlap, because the slice partition makes
  the macroblock sets disjoint;
* every consumer stays in safe code, with the bounds checks intact;
* it is mode-independent: no row-alignment precondition, so all four multi-threaded
  configurations use the same path, and so does the single-threaded one;
* Miri's data-race detector then reports an *actual* overlapping access rather
  than a retag, which is the check that means something.

Its cost is real and should be stated: the kernels that write the reconstruction
plane take `&mut PlaneCursorMut` and use `row_mut`, and `row_mut` cannot exist on
a shared view. Three families need a write-through-`&self` flavour —
`common/copy_mb.rs`'s seven `copy_WxH`, the IDCT-and-add family in
`encoder/decode_mb_aux.rs`, and `common/deblocking_common.rs`'s edge filters.
The census (`phase9_plane_census.md` §1) puts the consumer count at **32 blocked
call sites** — 17 copy, 10 idct, 5 intra-pred — plus deblocking's own walk.

**(B) A per-row block view.** `MbBlockMut { rows: [&mut [u8]; 16] }`, each row
slice exactly the macroblock's columns plus the deblocking skirt, derived per row
from a raw base. Those ranges *are* disjoint across workers, so no interior
mutability is needed and the `unsafe` is one constructor. It costs the same kernel
churn as (A) and loses the negative-offset reads, which intra prediction needs, so
(A) is the better shape unless (A)'s `unsafe impl Sync` is judged unacceptable.

**(C) Declare the reconstruction write a lawful boundary.** Keep the raw seam,
give it its own `unsafe-cat` category alongside `C-ABI`, and stop. This changes
the charter's exit condition 1 and is a decision for the steward, not for a
session — but it is the cheapest option and it should be named rather than
discovered late.

### 4. What session C inherits

Blocked consumers, by route (`phase9_plane_census.md` §6):

| route | sites |
|---|---:|
| `SPicData.pCsMb` / `pDecMb` | 42 |
| `SDqLayer.pCsData[..]` | 24 |
| `layer_dec_pic_mut` / `layer_dec_pic` | 14 |
| `SPicture::planes()` | 38 |
| blocked kernel call sites | 32 (17 copy, 10 idct, 5 intra-pred) |
| `deblocking.rs` / `common/deblocking_common.rs` | 20 + 13 `unsafe fn` |

**Acceptance**: the Miri probe
`fork_join_encodes_a_multi_slice_frame_under_the_aliasing_checker`
(`svc_encode_slice.rs:4160`) loses its `#[cfg_attr(miri, ignore)]` and passes.

**And that probe is not sufficient on its own.** It drives
`SM_FIXEDSLCNUM_SLICE` with `slice_num: 2` and RC on — which §1 measured as the
one **row-aligned** configuration, i.e. the easy case. A design that only works on
row-aligned slices would pass it. A second probe on a mid-row boundary is
required: `EncoderProbeOptions` already carries `slice_mode`, `slice_num` and
`slice_constraint` (`api/codec_api.rs:4305-4327`), so either
`SM_RASTER_SLICE` with an uneven `uiSliceMbNum[]` or `SM_SIZELIMITED_SLICE` with
`threads: 2` reaches it.

## F108 — deblocking runs **inside** the fork under multi-threading, not after the join

Session B's brief states, in the sentence that scopes the reconstruction-write
question: "Deblocking runs after the join." That is true of one of the two paths
and false of the one that matters.

The frame-level filter is guarded by `!bDeblockingParallelFlag`
(`encoder_ext.rs:3789-3795`), and `bDeblockingParallelFlag` is
`iMultipleThreadIdc != 1` (`wels_encoder_ext.rs:1319`). So under multi-threading
`PerformDeblockingFilter` **never runs**. Instead each worker deblocks its own
slice, inside the job body, immediately after `WriteSliceBs`:

```rust
// slice_multi_threading.rs:1384-1386, in EncodeOneSliceInJob
let pfDeblockingFilterSlice =
    (*ctx_func_list(pCtx)).pfDeblocking.pfDeblockingFilterSlice.unwrap();
pfDeblockingFilterSlice(current_layer(pCtx), ctx_func_list(pCtx), pSlice);
```

and the same three lines again in `EncodeOnePartitionSizeLimited`
(`:1668-1670`). `DeblockingFilterSliceAvcbase` resolves the reconstruction picture
with `layer_dec_pic_mut` and takes `planes()` off it (`deblocking.rs:1436-1442`)
— i.e. it is **one of the workers taking the `&mut SPicture` that F73 is about**,
not a single-threaded step that could be left alone.

Two consequences for F107's design:

1. Deblocking is a reconstruction **writer** inside the fork, so its 33 `unsafe fn`
   (20 in `encoder/deblocking.rs`, 13 in `common/deblocking_common.rs`) are part of
   the write-view conversion, not after it.
2. Its writes stay inside the worker's own macroblocks for the reason §2 gives —
   `uiFilterIdc = 1` suppresses both cross-slice edges — so it does not weaken the
   disjointness premise. It does mean the write view's rectangle is the macroblock
   **plus the deblocking skirt** (up to 4 samples back across the left and top
   edges), which shape (B) above has to size for and shape (A) does not.

## F109 — "resolve the handle at the point of use" is not always sound: `pDecPic` moves under the PSNR block

The plane family's whole design is *carry a handle, resolve it where you read the
pixels* — that is what replaces the raw cursor. T9.B3 converted the first site to
it (`WelsCalcPsnr`, the layer's PSNR against its source) and the conversion was
wrong on the first attempt, in a way no gate in this project would have reported.

The port carried the two pictures as `PicPlanes` copied out at the top of the
layer body:

```rust
let mut fsnr: Option<PicPlanes>;            // encoder_ext.rs:3193, was
...
    (*pCtx).pDecPic = (*pRefListCur).pNextBuffer;      // :3481
    fsnr = match (*pCtx).pDecPic { Some(id) => Some((*pRefListCur).pic_mut(id).planes()), .. };
...
    fSnrY = WelsCalcPsnr(sFsnr.pData[0], .., pEncPic.pData[0], .., w, h);   // :3885
```

The obvious conversion drops `fsnr` entirely and reads `(*pCtx).pDecPic` at the
PSNR block. **It names a different picture there.** Between the two lines sits

```rust
if eNalRefIdc != NRI_PRI_LOWEST && !eRefStrategy.UpdateRefList(pCtx) { .. }   // :3864
```

and `WelsUpdateRefList` ends with `eRefStrategy.EndofUpdateRefList(pCtx)`
(`ref_list_mgr_svc.rs:824`), which on the camera strategy is `PrefetchNextBuffer`
— whose last line is `(*pCtx).pDecPic = (*pRefList).pNextBuffer` (`:690`), i.e.
**the slot the *next* frame will decode into**. The PSNR would have measured an
unrelated picture.

The fix is to keep the snapshot and change only what is snapshotted: `fsnr` is now
`Option<RecPicId>` rather than `Option<PicPlanes>`. The handle is captured at the
same line, for the same reason, and the *picture* is resolved at the read.

Two things worth carrying forward:

1. **S37's rule needs a second half.** "A picture is an arena, so resolve it once,
   copy the geometry out, and do not hold a borrow across the calls" was written
   about *borrows*. What this site needed copying out was the **value** — which
   picture — and that is true whether the port holds a borrow, a raw root or a
   handle. Session B2 converts 120 `SPicData` reads to exactly this shape, so
   before each one: **is the field the handle comes from still the same field at
   the point of use?** `SPicData` is stamped per macroblock from the layer, and the
   layer's `pDecPic`/`pRefPic` are stable across a frame — but `(*pCtx).pDecPic` is
   not stable across a *layer body*, and that is the kind of difference a
   handle conversion silently eats.
2. **No gate covers this path.** Both diffharness drivers set
   `bPsnrY = bPsnrU = bPsnrV = false` (`cxx_enc.cpp:148`,
   `rust_enc/main.rs:133-135`) and no unit or integration test asks for PSNR, so
   the 535-configuration sweep is blind to the whole block. The conversion is
   defended by the kernel's own differential test — whose goldens are **measured
   against `libopenh264.a`**, so it survives the shim's deletion with meaning
   (contrast F106) — and by the fact that the two handles are the same two the raw
   roots were derived from. It is not defended by a byte gate, and that is worth
   saying plainly rather than letting "gates PASS" imply it.

## F110 — `WelsEncRecUV`'s `pRes` is **not** a function of `iUV`: the two callers use different bases

Session D's brief names `WelsEncRecUV` as the clearest case of "delete the second
path" and says why:

> its `pRes` is `coeff_level + 256 + (iUV - 1) * 64`, a pure function of the `iUV`
> argument it already receives, so the parameter is redundant.

It is not. The two callers pass **different bases**, and the C++ they were ported from
does the same:

```cpp
void WelsIMbChromaEncode (...) {                    // svc_encode_slice.cpp:469
  int16_t* pCurRS = pMbCache->pCoeffLevel;          // :475   — base 0
  ...
  WelsEncRecUV (pFunc, pCurMb, pMbCache, pCurRS,      1);
  WelsEncRecUV (pFunc, pCurMb, pMbCache, pCurRS + 64, 2);
}
void WelsPMbChromaEncode (...) {                    // :493
  int16_t* pCurRS = pMbCache->pCoeffLevel + 256;    // :499   — base 256
  ...
  WelsEncRecUV (pFunc, pCurMb, pMbCache, pCurRS,      1);
  WelsEncRecUV (pFunc, pCurMb, pMbCache, pCurRS + 64, 2);
}
```

Deriving `+ 256` inside the callee would have moved **every intra macroblock's chroma
residual by 512 bytes**, and the two paths are independent enough that a P-only sweep
would not have caught it.

**The generalisation is the useful part.** "Delete the second path" is right, and it
is about the *pointer*, not the information the pointer happened to carry. The
parameter here carries caller state — which half of `sCoeffLevel` this call works in —
and that state has to survive. It survives as a `usize` index, which no retag can
invalidate:

```rust
pub unsafe fn WelsEncRecUV(pFuncList: &SWelsFuncPtrList, pCurMb: &mut SMB,
                           pMbCache: &mut SMbCache, kiResOff: usize, iUV: i32)
```

**Before deleting a redundant-looking cursor parameter, check every caller's base, not
one caller's arithmetic.** Two of this family's callee parameters were genuinely
redundant (`WelsMdInterInit`'s and `AcceptPskip`'s arena, both derivable); this one
was not, and it looked identical from the callee's side.

## F111 — `q1c.py` was blind to `addr_of_mut!` and to a root minted from an owned field: 22 `SMbCache` sites and 20 context sites

The detector was rebuilt in T9.A2 from F66's written specification and has been the
phase's precondition instrument since. Aimed at `SMbCache` and run against the tree it
was about to gate, it had **two** false-negative classes, and each one hid live sites.

**(a) `DERIV_HINT` did not match `addr_of_mut!`.** It recognised `as_mut_ptr()`,
`.add()`, `&mut` and a pointer cast — but not the spelling this codebase *mandates*
for a cursor into an inline array. S28/S29 exist precisely to force

```rust
let pRes = std::ptr::addr_of_mut!((*pMbCache).sCoeffLevel).cast::<i16>();
```

and the detector scored that as "not a derivation". The effect was masked while the
ten accessors stood (their call sites read `md::coeff_level(pMbCache)`, which the hint
also missed, for a *different* reason — a bare call). T9.D4 replaced the accessors
with the mandated spelling, and the report went to **0 with nine live sites in it**.

**(b) A root can be an owned field, not just a parameter.** `body_roots` grew a local
into a root from an explicit `*mut T` type or cast, `Box::into_raw`, a `**T` deref, or
an alias. `SMbCache` is one owned field of `SSlice` and **32 bodies** mint their root
as `let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);`, whose type appears
nowhere on the line — so all 32 were invisible *as callers*. Reading the field's
declaration out of the tree (`pub sMbCacheInfo: SMbCache,`) supplies the type.
`WelsWriteMbResidualCabac` alone held 12 of the sites this uncovered.

Together: `--type SMbCache` went **0 -> 22 sites in 4 callers**, and the default
`sWelsEncCtx` scan went **288 -> 308 sites, 68 -> 71 callers, 84 -> 86 callees** (all
twenty from (a); the context is a parameter everywhere, so (b) adds nothing there).

**Family 6's precondition number moves with this.** The charter §4.6 and §8 both quote
"287 sites, 68 callers"; the tree at session D's close reports **308 / 71**, and the
figure was 288 before this fix — so the charter was also one behind on the old
measure.

Three limits remain, and they are in the tool's docstring rather than only here:

* a bare `let p = accessor(pArena);` is still not a derivation to the hint. Widening
  it to "any call taking a root" was measured and not taken: on the context struct it
  matches `let pCurLayer = current_layer(pCtx);` and every sibling accessor and would
  swamp the signal. **The mitigation is structural** — session D's conversion deleted
  every such accessor, so the shape has no instances left in this family.
* calls made **through a dispatch slot** are not attributed to any callee, because the
  scan matches call sites by name. Session D's three arena-taking slots were checked by
  hand. This is the same blind spot F111's sibling finding hits from the other side —
  see the `SMB` note in T9.D9.
* it models exactly one conversion, "the parameter becomes `&mut T`". A callee
  *narrowed* to the field it touches never had the hazard the report attributes to it;
  ten of session D's first twenty-one sites were of that kind.

`--kind ref` was added for the other half of the instrument's life: once a family has
converted, the same scan answers "is a cursor held across one of these calls **today**",
and without it a converted family reads as "nothing to find" and exits 2.

## F112 — the arena's roots must stay raw: a `&mut` to an owned field is popped by the callee that re-derives it

T9.D7 converted all 41 `*mut SMbCache` parameters to `&mut SMbCache`. The obvious
companion change — the 32 roots —

```rust
let pMbCache = &mut (*pSlice).sMbCacheInfo;      // instead of addr_of_mut!
```

was written, gated green, and **reverted**, because the tree already carries the
counterexample. T6.C2 found it with a Miri encode probe and wrote it into
`WelsSpatialWriteMbSyn`:

> `WelsSpatialWriteMbPred` and `WelsSpatialWriteSubMbPred` re-derive both the frame
> buffer and the slice's `sMbCacheInfo` for themselves, so a `&mut` of either taken
> before Step 1 is invalidated by Step 1 and used again in Steps 2-4. **This is not a
> spelling: the borrow has to be taken after the call that pops it.**

The reason generalises past that one function. Stacked Borrows is **per byte**, so:

* a field read or write *through* `pMbCache` touches only that field's bytes and
  leaves cursors into other fields alone — which is why a `&mut SMbCache` parameter is
  affordable at all, and why the four half-selectors can read their flag while the
  arrays are under cursors;
* **entering** a function that takes `&mut SMbCache` retags every byte of the arena
  and kills every outstanding cursor — F66's rule, and what `q1c.py` measures;
* and a **sibling** derivation from the shared parent pops whatever sits above that
  parent for those bytes. Two raw siblings coexist; a `&mut` sibling does not. These
  functions nest — `WelsMdInterMbLoop` -> ... -> `WelsSpatialWriteMbSyn` -> the two
  writers — so with `&mut` roots the inner body's root pops the outer body's, and the
  outer uses it afterwards.

Measured, with the detector aimed at the slice: `q1c.py --type SSlice` reports **68
hazardous sites in 22 callers** both before session D and after it — unchanged,
because the roots are unchanged. That number is what a `&mut` root would have had to
answer for.

**So the arena is a reference at every *parameter* and a raw at every *root*, and the
40 call sites in between spell `&mut *pMbCache`** — a Unique that lives for the call
and dies there. The root conversion is not this family's: it belongs to whoever
converts `*mut SSlice`, where it is one step rather than thirty-two.

## F113 — the residual slots' raw types were hiding three things, and one of them served two lengths

T9.D8 and T9.D10 re-typed **all fourteen** of F103's slot-only coefficient kernels from
`unsafe extern "C" fn(*mut i16, ..)` to safe function pointers over exact arrays. Three facts the raw types had been
concealing, each of which had to be resolved before the type could be written:

0. **T9.D8 got the boundary wrong, and the fix is the finding's other half.** It
   flipped eleven and left the three dequantisers, calling them "a different typedef
   family shared with the decoder side". They are not:
   `encoder/decode_mb_aux.rs` is the *encoder's* reconstruction-side dequantisation,
   `grep pfDequantization src/decoder` is empty, and the three take a coefficient span
   and a `g_kuiDequantCoeff` row with no plane operand. T9.D10 flipped them.
   **A "shared with the other codec" claim is a grep, not an inference from a
   directory name** — and `decode_mb_aux.rs` sits under `src/encoder/`.
1. **One `PQuantizationFunc` served two different spans.** `pfQuantization4x4` quantises
   one 4x4 block and `pfQuantizationFour4x4` quantises four, and both had the same
   type, because `*mut i16` says nothing about length. They are `PQuantization4x4Func`
   over `[i16; 16]` and `PQuantizationFunc` over `[i16; 64]` now. A raw pointer that
   serves two lengths is a type that was not saying what it meant.
2. **The odd array lengths are the *reach*, not the block.** `hadamard_quant_2x2` and
   its `Skip` twin read `rs[0]`, `rs[16]`, `rs[32]`, `rs[48]` — span **49**, not 64 —
   and `hadamard_t4_dc` reads block 15's DC at index 240 — span **241**. Both numbers
   were already pinned by the T7 differential test's allocations; they are in the
   signatures now, where a caller can see them.
3. **`aMax` was `[u16; 4]` cast to `*mut i16` at two call sites.** The kernel's
   `max_abs` starts at 0 and only ever grows, so every entry is non-negative and the
   `u16` and `i16` readings of those bits agree — but nothing in the raw signature said
   so, and the two spellings would have diverged on the first negative value. The array
   is `i16`, which is what was always stored in it.

**S28 does not object to the index helpers.** The rule is about a *raw* cursor taken
through a sub-slice, whose tag dies the moment the kernel walks past the slice — and
these kernels used to walk. A safe borrow of `sCoeffLevel[off .. off + 16]` is taken
afresh per block, lives exactly as long as the call it is made for, and the kernel's
own signature says it cannot read outside it.

**And the T7 differential test's quant/scan/Hadamard rows became tautological** the
moment the shims were deleted: both sides are the same code. Per F106 they are deleted
rather than re-pinned, and replaced by relations that were never tautological —
`quant_four_4x4` is `quant_4x4` per quadrant, `quant_four_4x4_max` is `quant_four_4x4`
plus a per-quadrant maximum, `scan_4x4_ac` is `scan_4x4_dc_ac` shifted by one, a scan
is a permutation of its input, the 2x2 Hadamard zeroes its four DC slots and touches
nothing else.

One replacement assertion was **wrong** and is worth keeping as the counterexample:
"`skip` says nothing survives **iff** the full kernel counted zero" failed on the first
random input. `hadamard_quant_2x2_skip` thresholds on `(1<<16 - 1) / mf - ff` in `i32`
while `hadamard_quant_2x2` truncates each butterfly output to `i16` before quantising,
so the two predicates agree on every input the encoder produces and are not the same
predicate. **A replacement for a deleted tautology is a new claim about the code and
needs the same evidence as any other** — it is out, with the reason recorded where it
stood.

## F114 — the arena conversion introduced two aliasing defects no byte gate could see, and each one changed a rule's precondition rather than breaking it

Session D's brief forbade Miri (hard rule 3: it "runs once at the end of the whole
phase"). Run at the user's direction over the finished session, the encoder-scoped
`--lib` step found **two** Undefined Behaviour reports, in two different mechanisms,
both introduced *by this session*. Neither was visible to anything the brief did
allow: at the moment they were found the tree had **535/535 diffharness sweeps in both
profiles, both benches bit-identical, 1089 tests green and a ratchet down on every
metric**.

Recording them together because the pair makes one point: **converting a raw parameter
to a reference does not only retag — it changes what every other derivation under that
parameter means, and it adds a protector that the raw never had.**

### (a) `addr_of_mut!` stops rescuing a derivation when the parent stops being raw

```
error: Undefined Behavior: trying to retag from <84500493> for SharedReadOnly
  permission at alloc16772982[0x150], but that tag does not exist in the borrow stack
  --> src/encoder/svc_encode_mb.rs:360        (inside WelsIDctT4Rec_c)
help: <84500493> was created by a SharedReadWrite retag at offsets [0x150..0x450]
  --> src/encoder/svc_encode_mb.rs:650
      let pResI4x4 = std::ptr::addr_of_mut!((*pMbCache).sCoeffLevel).cast::<i16>();
help: <84500493> was later invalidated at offsets [0x150..0x450] by a Unique retag
  --> src/encoder/svc_encode_mb.rs:684
      func(blk4x4_mut(&mut (*pMbCache).sCoeffLevel, 0), pFF, pMF);
```

reached through the real encode loop: `WelsEncRecI4x4Y` <- `WelsMdI4x4Fast` <-
`WelsMdIntraFinePartitionVaa` <- ... <- `WelsEncoderEncodeExt`.

**The rule that moved.** T5.O8 already wrote the general form (`safety_refactor_log.md`):

> `addr_of_mut!` **is not a charm** ... It rescues a derivation whose invalidating
> write goes through a *raw* parent — a raw sibling does not pop a raw derivation.

That is exactly right and it is exactly why F70's `InitSliceSettings` site is sound:
its parent `pDlp` is a `*mut SSpatialLayerConfig`, so `addr_of_mut!` on a field reuses
the parent's `SharedReadWrite` item and a sibling `&mut` pushes above it without
popping it. **T9.D7 removed that precondition without noticing**: once `pMbCache` is
`&mut SMbCache`, the same `addr_of_mut!` gets *its own* item above the parent's
`Unique` — Miri says so in as many words, "created by a SharedReadWrite retag" — and a
sibling `&mut` derived from that same `Unique` pops it. T9.D8 then introduced the
sibling, by giving the residual slots safe array parameters.

Two details worth carrying:

* **The `&mut` covers the whole field, not the sub-range.** `blk4x4_mut` takes
  `&mut [i16]`, so the caller borrows all 384 coefficients before indexing —
  `[0x150..0x450]` in the report. Narrowing the *helper's* parameter would not help;
  the borrow is taken at the call site.
* **Four bodies had the shape, and only one was reported**, because Miri aborts on the
  first error. `WelsEncRecI4x4Y` (the one reported), `WelsEncRecI16x16Y` (sixteen DC
  stores plus `pfIDctFourT4`), `WelsEncInterY` (two `WelsSetMemZero_c` calls) and
  `WelsEncRecUV` (one memset plus four DC stores). A grep for the shape — a body
  holding a bound `addr_of_mut!((*root).field)` across a `&(mut) (*root).field` and
  using it after — found those four and nothing else in `src/encoder`.

**Fix:** within a body, the field is reached one way. The DC write-backs and the three
memsets became indexed writes and `fill(0)`; the raw that the still-raw plane-taking
slots (`pfDctT4`, `pfIDctT4`, `pfIDctFourT4`, `WelsDctMb`) require is derived **at the
call** rather than held across the borrows. That removes raw pointers rather than
adding any.

### (b) a reference *argument* is a protector; a raw parameter never was

```
error: Undefined Behavior: not granting access to tag <90263396> because that would
  remove [SharedReadOnly for <93029106>] which is strongly protected
  --> src/encoder/svc_encode_slice.rs:1390
      UpdateMbNeighbor(pCurDq, &mut *pMb, kiMbWidth, ...);
help: <93029106> is this argument
  --> src/encoder/svc_encode_slice.rs:3056        pCurMb: &SMB,   (AddSliceBoundary)
```

T9.D9 converted `AddSliceBoundary` and `DynSlcJudgeSliceBoundaryStepBack` from
`*mut SMB` to `&SMB` on the grounds that they only *read* the macroblock — which is
true, and beside the point. A `&`/`&mut` passed as a **function argument** is
*strongly protected* for the duration of that call: nothing may invalidate it, even
transiently. `AddSliceBoundary` calls `UpdateMbNeighbourInfoForNextSlice`, which walks
the macroblock list and takes `&mut` to each element — including the one the protected
argument names.

**The conversion created this. It did not expose it.** With `*mut SMB` there is no
protector and no conflict; the C++ has neither. This is the counterpart to F66's rule
(a `&mut` parameter *retags*) and it is not the same rule: a **shared** reference
retags harmlessly and still protects.

**Fix:** both functions read exactly one field, `(*pCurMb).iMbXY`. The parameter is an
`i32` now — two pointer parameters deleted rather than reverted, and no protector to
conflict with.

### What this says about the session's gates, and about the next one

* **A byte gate cannot see aliasing, and this is the sharpest instance the project has
  produced.** 535 sweep rows twice, two bit-identical benches and 1089 tests all passed
  over both defects. **This is what D-gate-4 was written on** (`prompts/phase9.md` §6,
  2026-08-22): every Phase 9 session now closes on `MIRI_SCOPE=encoder gates.sh
  session`, a level that adds the encoder-scoped Miri `--lib` step to the sweeps and
  leaves the benches at the phase close. `q1c.py` could not see either: (a) is a raw invalidated by a
  *safe* borrow, which the detector does not model at all, and (b) is a protector,
  which it does not model either.
* **The detector's model needs a third shape.** It knows shape A (a cursor held across
  a call that retags) and shape B (argument evaluation order). These are shape C — a
  raw held across a *safe borrow of the same field* — and shape D — a reference
  argument protected across a call that re-borrows what it names. Both are cheap to
  scan for; neither is implemented.
* **The precondition audit generalises.** Both defects are a rule whose *precondition*
  a conversion silently removed. Before converting a parameter to a reference, the
  question is not only "does anything hold a cursor across a call to it" (F66) but
  **"what did every derivation under this parameter mean while it was raw, and does it
  still mean that?"**

## F115 — three mode-decision functions are dead in the port and `#if 0` upstream, and deleting them is a decision rather than a sweep

`WelsMdP4x4`, `WelsMdP8x4` and `WelsMdP4x8` (`svc_base_layer_md.rs:1084`, `:1154`,
`:1224`) have **no caller anywhere in the port** — roughly 210 lines, 4 tagged
`#[allow(unsafe_code)]` items, and 6 `SPicData` reads between them (3 `pEncMb`, 3
`pRefMb`). Upstream calls them only from inside

    #if 0 //Disable for sub8x8 modes for now

(`codec/encoder/core/src/svc_mode_decision.cpp:635-655`).

Recorded rather than acted on, and the reason is the one the session brief gives:
"dead in the port" and "dead upstream" are different claims. The first is a grep and
this session ran it; the second is a statement about what the codec is *for*, and a
future session that wants sub-8x8 partitions back would find the port had quietly
decided against it. **Deleting them is the user's call.** If taken, it removes 6 of
the 71 `pEncMb`/`pRefMb` reads that session C would otherwise convert, which is the
only reason it is worth raising at all.

## F116 — the plane census had drifted 40 sites into `?` and exited 0, and the document it feeds was right the whole time

Session D (T9.D7) deleted the ten `md::mem_pred_*` / `skip_mb` / `coeff_level`
accessors in favour of reaching the field directly. `phase9_plane_callers.py`
classifies an operand by **how it is spelled**, so nothing failed: 40 call sites
moved from `src`/`cache`/`coeff` into `?`, the coefficient column went from 13 to 0,
the tool printed the new numbers without comment and **exited 0**. Session B2's own
brief scoped against those numbers and inherited them as fact.

Four repairs, all in T9.B20 — the field spellings (`sCoeffLevel` alone was 23 of the
40), receiver-agnostic `SPicData` rules (`AcceptPskip` takes the bundle as
`kpPicData: &SPicData`, so its one `.pEncMb[0]` operand was unclassifiable for that
reason alone), a `[` added to `root_ident`'s strip set (the alias walk dead-ended on
the array literal `[pMemPredChroma, ..]`, one hop short of what classifies it), and
**a non-zero unclassified count now exits 1** with a stderr banner.

**The check that makes this a finding rather than a fix.** The regenerated §1 tables
are *byte-identical* to the census session B committed. The document was correct
throughout; only the instrument had drifted, and the drift was invisible precisely
because the tool reported it as a clean run. S46 (an empty scan must be loud) and S55
(calibrate a detector on a known positive) are the same lesson; this is the third,
and the new rule is the general one: **a reading aid that sessions scope against is a
gate whether or not anyone calls it one.**

## F117 — the source picture *is* written during the macroblock loop, in the opposite direction from the one scoped, and no gate can see the path

`phase9_plane_census.md` claimed `src` is "read-only for the whole frame — any number
of shared `PlaneCursor`s, on any number of threads, is sound". Session B2's brief
disputed that and named `VaaBackgroundMbDataUpdate` (`svc_mode_decision.rs:510`,
called from `WelsMdBackgroundMbEnc` inside the macroblock loop and in-fork), reading
the three copies as writing the *previous* source picture:

    copy16((*pVaaInfo).pCurY.offset(kiOffsetY), kiPicStride,
           (*pVaaInfo).pRefY.offset(kiOffsetY), kiPicStride);   // "dst is pRefY"

**The direction is backwards.** `PCopyFunc` is `(pDst, iStrideD, pSrc, iStrideS)`
(`md.rs:235`), so the first argument is the destination: the copy runs *previous
source* → *current source*, and the C++ is the same call in the same order
(`svc_base_layer_md.cpp:1347`).

That makes the aliasing **worse than scoped, not better**. The brief framed it as two
elements of one pool needing index-disjoint access. Measured, the destination is the
picture the encoder is reading: a probe at `WelsInitCurrentLayer` comparing
`(*pCtx).pEncPic` against `GetCurrentOrigFrame(did)` reported `same:true` on **18
frames of 18**. So the write target is *the picture `SPicData.pEncMb` reads*. No
`pair_mut`-style pool accessor helps — the conflict is read-vs-write on one element,
not between two.

**And nothing in the gate battery can see it.** The diffharness driver sets
`p.bEnableBackgroundDetection = false` (`tools/diffharness/rust_enc/main.rs:126`), so
`BackgroundDetection` is never called in any sweep preset in either profile — a probe
inside it printed nothing across a full 18-frame run, and `pVaaInfo->pCurY` is still
null when the layer is stamped. The path is outside the 535 sweep rows *and* outside
the encoder-scoped Miri step.

**Decision (T9.B20): the three copy sites stay raw and tagged**, for session C to take
with the rest of the write-under-shared-view problem (D-mt-3). Converting code that no
gate exercises, on the one aliasing shape in this family that is genuinely hard, is
the wrong trade at any price. The general rule this yields is **S57**: before
converting a site, ask whether any gate runs it — and if none does, that is an
argument for leaving it raw, not for converting it carefully.

## F118 — a `PlaneCursor` cannot feed a raw slot, so the carrier conversion and the table flip are one commit, not two phases

Session B2's plan had step 2 (every source and reference reader builds a
`PlaneCursor`) preceding step 4 (the cost tables flip). **The two cannot be
sequential**, and the reason is a missing method rather than a policy: `PlaneCursor`
exposes no `as_ptr`, and `PaddedPlane::root_ptr` — the only raw escape in
`safe/plane.rs` — takes `&mut self`, so calling it per macroblock is F73's
whole-picture retag. A converted reader therefore has nothing to hand a
`unsafe extern "C" fn(*mut u8, i32, *mut u8, i32)` slot, and an unconverted one has
nothing to hand a safe one. For any given call site the operand and the slot move
together or not at all — which is S20's closure, stated for this family.

The consequence is not that the work is bigger; it is that **the unit is different**.
Two facts make it tractable:

* **Motion compensation needs no slot at all.** Phase 4a made `McLuma_c`/`McChroma_c`
  direct calls, so MC converts *caller by caller* with no table involved. T9.B22 is
  the first one and the shape it proves is reusable.
* **The cost tables are constant after init.** `WelsInitSampleSadFunc`
  (`sample.rs:332`) is the only writer of `pfSampleSad`/`pfSampleSatd`/`pfSample4Sad`
  — its `_uiCpuFlag` parameter is unused and every other mention in the tree is a
  read. So a call site whose block index is a **compile-time constant** may call
  `sample_sad::<16,16>` directly and byte-identically, bypassing the slot entirely,
  exactly as MC does. That decouples most of the 37 sad/satd sites from the one
  type-change that `md_cost`/`me_cost` and the 42 handoff signatures would otherwise
  force into a single commit. The runtime-selected sites (`md_cost`/`me_cost` choose
  by `CostFamily`) still need a safe table, but they are 13 of 37, not all of them.

## F119 — the 22 "dead" `mc.rs` shims are not dead: a differential test is their caller, and the grep that found them excluded it

Session B2's step 5 lists 22 `mc.rs` shims to delete under S18, on the evidence of
`grep -rn '\bMcHorVer01_c\b' src/encoder src/decoder src/processing src/api` per name.
Every one of those greps is correct and every one of them scopes out `tests/`.

`tests/kernels_differential_phase2.rs` names **13** of them — `McCopy_c`,
`McHorizLuma_c`, `McVertLuma_c`, `McHorVer01_c`, the four `McCopyWidthEq*_c` and
`McChromaWithFragMv_c` among them — as the raw entry points of the phase-2 kernel
differential harness, which drives each shim against the C++ reference across a size
and reach matrix. They are dead to the *encoder*; they are live as the port's
kernel-level parity evidence.

So deleting them is not a sweep. It trades ~22 tagged raw signatures for the
differential coverage that proves those kernels match C++ — and the harness compares
through a raw-pointer ABI, so the safe kernels cannot simply be substituted. Recorded
undone for the same reason as F115: the cost is coverage, and what coverage is worth
keeping is not a decision a conversion session should make silently.

Two things confirmed while checking, so the next session need not re-do them: none of
the 22 is `#[no_mangle]` or otherwise ABI-exported, and `McLuma_c` does **not**
dispatch to the sixteen `McHorVer**_c` — it calls the safe `mc_luma` through
`shim_wh`, so the "inside=3..7" occurrence counts are doc comments plus the
definition. The brief's reachability claim was right; only its search path was short.

## F120 — the plane census had never counted `md.rs`'s ten motion-compensation sites, and the brief and the charter disagreed about the number without either being right

Session B3's brief says the plane family has **30** motion-compensation call sites
(33 minus T9.B22's three); the charter's B3 row says **20**; `phase9_plane_callers.py`
printed 20. All three numbers came from the same instrument in different states, and
the tree has 30.

`SHIMS` knew two `common/mc.rs` entry points — `McLuma_c` and `McChroma_c` — and
nothing else, so the ten direct calls the fractional refinement makes were invisible
to it: `MeRefineFracPixel`'s `McHorVer02_c`, `McHorVer20_c` and four `McHorVer22_c`,
and `MeRefineQuarPixel`'s four `PixelAvg_c` (`encoder/md.rs`). Those are not table
slots (Phase 4a made MC direct) and not composition calls inside a kernel — they are
exactly the caller-side sites the census exists to enumerate.

Teaching it the four names (T9.B24) moved the total 20 → 30 and `md.rs` 10 → 20; a
`PARAM_CLASS`-style pair of classifier arms went in with them for
`SQuarRefineParams::pSrcA`/`pSrcB`, which an alias walk cannot follow because they are
fields of a parameter.

**The lesson is S58's, one turn further on.** F116 made the tool fail on an operand it
cannot classify. That does not cover an operand it never looks at: a *missing entry
point* is silent by construction, because there is no site to leave unclassified.
`SHIMS`/`SLOTS` are as load-bearing as the classifiers, and the check that finds a
hole in them is not a run of the tool — it is a grep for the kernel's name over
`src/`, compared against the census's own row count, done when the family is scoped.

## F121 — `CalUVSadCost` and its two callers are dark, and so are the two `planes()` retags session B2 flagged as live

Session B3's brief lists `CalUVSadCost` + `JudgeStaticSkip`/`JudgeScrollSkip` as step
1 item 3, and names their `ctx_pic_ref_mut(..).planes()` calls as "F73's retag already
in the tree" (session B2's trap (c)). **No gate runs any of it.**

`CalUVSadCost` has exactly two callers, both reached only through
`pfSCDPSkipDecision`, which `WelsInitSCDPskipFunc` points at the judging arm only when

    bScreenContent && bEnableSceneChangeDetect && iComplexityMode < HIGH_COMPLEXITY

(`encoder_context.rs:1574`). The diffharness driver encodes as
`CAMERA_VIDEO_REAL_TIME`, so `bScreenContent` is false in every preset in both
profiles and the slot is `WelsMdInterJudgeSCDPskipFalse`.

Measured with a probe that prints once per entry, **against a calibration probe in
`WelsMdI16x16`** — a zero from an uncalibrated instrument is not a zero (S46/S55),
and the first run of this probe read zero for *everything* because `compare.sh` sends
the driver's stderr to a log file rather than to its own output:

| configuration | `WelsMdI16x16` | `WelsMdBackgroundMbEnc` | `SvcMdSCDMbEnc` | `JudgeStaticSkip` | `JudgeScrollSkip` |
|---|---:|---:|---:|---:|---:|
| 320x192 CAVLC qp26 | 2008 | 0 | 0 | 0 | 0 |
| 320x192 CABAC complexity HIGH | 1882 | 0 | 0 | 0 | 0 |
| 160x96 gop4 rc1 | 300 | 0 | 0 | 0 | 0 |
| Static_152_100 rc0 | 377 | 0 | 0 | 0 | 0 |
| 320x192 `sm=1 t=4` | 2136 | 0 | 0 | 0 | 0 |

So the four bodies stay raw under S57 and each carries the measurement in its doc
comment (T9.B27), which is what S57 asks for — the answer beside the site, so the next
session does not re-derive it.

**The part worth generalising** is not that these are dark; F117 already established
the shape. It is that **a hazard in dark code is not a hazard**. Session B2's brief
raised the two `planes()` sites as a live F73 retag needing repair; they are live as
*code* and unreachable as *behaviour*, and repairing them would have been the same
trade S57 forbids for conversions — work whose correctness nothing can check. The
question "which gate runs this" belongs to hazard triage as much as to conversion
scoping.

## F122 — three of `WelsMdInterMbRefinement`'s seven arms are unreachable: D-dead-1 deleted the producers and left the consumers

`WelsMdInterMbRefinement` dispatches on `uiSubMbType[i]` into four sub-8x8 arms —
`SUB_MB_TYPE_8x8`, `SUB_MB_TYPE_4x4`, `SUB_MB_TYPE_8x4`, `SUB_MB_TYPE_4x8` — and the
last three cannot be reached. Every writer of `uiSubMbType` in the port sets
`SUB_MB_TYPE_8x8` (`svc_base_layer_md.rs:1120`, `:1205`, `:1218`,
`svc_mode_decision.rs:2361`), because the sub-8x8 *search* is the `#if 0` block
D-dead-1 deleted `WelsMdP4x4`/`WelsMdP8x4`/`WelsMdP4x8` for.

Grepped, then probed — one print per arm entry, three configurations:

| arm | 320x192 CAVLC | 160x96 gop4 | 320x192 `t=4` |
|---|---:|---:|---:|
| `MB_TYPE_16x16` | 1005 | 127 | 681 |
| `MB_TYPE_16x8` | 144 | 12 | 120 |
| `MB_TYPE_8x16` | 202 | 24 | 138 |
| `SUB_MB_TYPE_8x8` | 264 | 68 | 540 |
| `SUB_MB_TYPE_4x4` | **0** | **0** | **0** |
| `SUB_MB_TYPE_8x4` | **0** | **0** | **0** |
| `SUB_MB_TYPE_4x8` | **0** | **0** | **0** |

T9.B28 converted all seven arms — four with a byte gate behind them and three
without. Recorded rather than reverted: the three are byte-for-byte the same
transformation as the four beside them, and re-raising them would leave a raw
spelling in the middle of a converted body for no gain. **What is recorded is that
their evidence is weaker than the commit's headline**, and that the same is true of
`UpdateP4x4MotionInfo`/`UpdateP8x4MotionInfo`/`UpdateP4x8MotionInfo`, the
`sMe4x4`/`sMe8x4`/`sMe4x8` members of `SWelsMD`, and the `SUB_MB_TYPE_*` arms of the
CAVLC and CABAC writers.

**The generalisation for S18**: a dead-code deletion is a *closure* question, not a
name question. D-dead-1 asked "does anything call these three functions"; the
question that would have found the rest is "what becomes unreachable when they go" —
the consumers of the values only they produced. The straggler sweep S18 runs at a
phase exit is the same computation done late; running it *with* the deletion is
cheaper than discovering the remainder two sessions later.

## F123 — `q1c.py`'s shape C reported shared borrows as hazards, and the fix is the same calibration that introduced it

`safe_borrows_of_field` matched `&(mut)? (*root).field` — both spellings — and shape C
labelled every hit `&mut (*root).field`. So the scanner flagged `WelsMdI16x16`'s

    &(*pMbCache).sMemPredMb[kiDstOff..][..256]

beside the raw `pPredI16x16` derived from the same field, and called it F114a.

It is not. Shape C models a `Unique` retag popping a sibling `SharedReadWrite` raw.
A **shared** reborrow of the field is a *read* through the parent, and a read access
does not remove `SharedReadWrite` items above the tag it reaches through — the raw
survives, the `SharedReadOnly` is pushed above it, and the next write through the raw
pops that in turn. F114a's own site was `blk4x4_mut(&mut (*pMbCache).sCoeffLevel, k)`,
a `&mut`, and the rule as written in S29's clause says `&mut`.

Narrowed to `&mut` in T9.B30, and **calibrated exactly as the shape was when it was
added** — against `0bfc7687^`, the tree that carried F114a's two live defects. It
still reports precisely four sites, `WelsEncRecI16x16Y` / `WelsEncRecI4x4Y` /
`WelsEncInterY` / `WelsEncRecUV`, with the Miri-reported one line-for-line (derive
`svc_encode_mb.rs:650`, borrow `:684`, use `:710`).

**S55, from the other side.** Its clause is about believing a zero from a blind
instrument. The mirror is believing nothing from a noisy one: a scanner that fires on
a sound spelling trains the session to argue with it, and the next real hit gets the
same argument. The remedy is identical — a change to a detector, in either direction,
is re-fired against the tree it was calibrated on before it is trusted.

## F124 — the D-cov-1 correction is right about the C++ and wrong about the count: **two** mc tests survive, and the second is Phase 4a's, not Phase 2's

Session B3's brief re-opened D-cov-1 and corrected it: the phase-2 kernel harness has
no C++ side, the mc equivalences were proven at `46053993` and deleted by the file's
own doctrine, and "the **only** surviving mc test is
`mc_shims_stay_inside_the_spans_they_declare` … a contract test of the shims' own span
arithmetic, which tests nothing once the shims are gone."

The first half holds and is confirmed here: `tests/kernels_differential_phase2.rs`
links nothing (`[dependencies]` is `libc` alone, there is no `build.rs` and no
`links` key), its every comparison is Rust-against-Rust, and its header states the
delete-on-shim doctrine in its own words.

The count does not. **`mc_table_slots_match_the_direct_calls`
(`tests/kernels_differential_phase2.rs:2343`) is a second surviving mc test**, and it
is a different kind of evidence: Phase 4a's dispatch assert-map, driving `SMcFunc`'s
six installed slots — `pMcLumaFunc`, `pMcChromaFunc`, `pfLumaHalfpelHor`/`Ver`/`Cen`,
`pfSampleAveraging` — against the symbols the de-virtualized call sites name, over
every quarter-pel and eighth-pel selector. Its own comment explains why it must be
behavioural rather than an address comparison (`#[inline(always)]` shims are
instantiated per codegen unit). It dies with the shims too, but deleting it spends
Phase 4a's mitigation, not Phase 2's span discipline, and the two should be retired
with their own reasons.

**And step 5 cannot complete as scoped, for a reason neither version of D-cov-1
names.** `common/mc.rs` reaches `#![deny(unsafe_code)]` only when *no* raw entry point
has a caller. After T9.B29 the four half-pel/average shims have no `src/` caller left,
but `McLuma_c` and `McChroma_c` still have six — the three in `WelsMdBackgroundMbEnc`
and the three in `SvcMdSCDMbEnc`, which F121 measures as dark and S57 keeps raw. So
the deletion splits: **22 dead shims + the two tests can go now; `deny` waits on a
background/screen-content referee**, which is step 6's preset and session C's problem.

### F121's postscript — the split, counted the tool's way

The first draft of this session's log put the 25 remaining `safe-now` sites at
"18 dark + 6 ME + 1 preprocess". Re-derived from `--sites` rather than from memory
it is **17 + 7 + 1**, and the two errors are worth naming because they are the two
mistakes this whole session was about.

* `CalUVSadCost` is **one** census row and **four** invocations — its two callers
  each call it twice. The census's unit is the *kernel call site*, so counting
  invocations inflates it. (S24: a count that decides anything comes from the
  instrument, in the instrument's own unit.)
* `WelsMotionEstimateSearchStatic`, `WelsMotionEstimateSearchScrolled` and
  `LineFullSearch_c` are **dark**, not ME-blocked: all three are behind
  `bScreenContent` (`WelsInitMeFunc` installs `pfVerticalFullSearch`/
  `pfHorizontalFullSearch` only there, and the first two are already tagged
  `SCREEN_CONTENT(dormant)`). The genuinely lit, ME-blocked sites are
  `WelsDiamondSearch`'s five and `WelsMotionEstimateInitialPoint`'s two.

So the ME struct's conversion (session E) unblocks **7** sites, and the
background/screen-content referee (session C) unblocks **17** — a ratio that is the
opposite way round from the draft, and it changes which of the two is worth doing
first.

## F125 — the 17 dark sites are two families, not one: the `bg` preset can light 8 and can never light 9, because `bScreenContent` is in the SCD conjunction

B3's close said its unfinished referee "unblocks 17 sites". Re-derived here from
`python3 rust/tools/phase9_plane_callers.py --sites` at `0bf9e0c9`, the 25 remaining
`safe-now` census rows split by *which flag* installs their dispatcher, and the two
flags are not the same flag:

| owner | sites | installed by | `bg` preset |
|---|---:|---|---|
| `WelsMdBackgroundMbEnc` | 5 (`svc_mode_decision.rs:600,601,602,607,662`) | `WelsInitBGDFunc(&mut *fl, (*pParam).bEnableBackgroundDetection)` | **lit** |
| `VaaBackgroundMbDataUpdate` | 3 (`:522,530,536`) | same | **lit** |
| `SvcMdSCDMbEnc` | 5 (`:2037,2047,2057,2072,2112`) | `WelsInitSCDPskipFunc` | never |
| `CalUVSadCost` | 1 (`:1843`) | same, via `JudgeStaticSkip`/`JudgeScrollSkip` | never |
| `WelsMotionEstimateSearchStatic` / `..Scrolled` / `LineFullSearch_c` | 3 (`svc_motion_estimate.rs:639,678,1054`) | `WelsInitMeFunc(..., bScreenContent)` | never |
| `WelsDiamondSearch` + `WelsMotionEstimateInitialPoint` | 7 (`:909,911-914,734,751`) | always installed | lit, ME-blocked |
| `Process` (`processing/scene_change_detection.rs:87`) | 1 | preprocess | out of scope |

The sentence that decides it is `encoder_context.rs:1607-1612`:

```rust
crate::encoder::svc_mode_decision::WelsInitSCDPskipFunc(
    &mut *fl,
    bScreenContent
        && (*pParam).bEnableSceneChangeDetect
        && ((*ctx_param(pEncCtx)).iComplexityMode as i32)
            < (crate::api::codec_api::ECOMPLEXITY_MODE::HIGH_COMPLEXITY as i32),
);
```

`bScreenContent` is a conjunct, and it is `iUsageType == SCREEN_CONTENT_REAL_TIME` —
an axis neither diffharness driver expresses. So **no camera-usage preset can ever
reach the SCD family**, however many background flags it sets. The referee unblocks
**8**, not 17; the other **9 are Phase 10's family mis-filed in Phase 9's plate**, and
their correct treatment here is a retag to `SCREEN_CONTENT(dormant)`, not a
conversion. `bEnableSceneChangeDetect` was not the missing knob and adding it would
have proved nothing.

**Measured, not argued.** With the `bg` preset finished and a probe in
`SvcMdSCDMbEnc`'s body, every `bg` row — including the ones where
`WelsMdBackgroundMbEnc` enters 5771 times — reads **`SvcMdSCDMbEnc` = 0**. See F126's
table.

## F126 — the `bg` preset's calibration, and the clip that lights the family without refereeing it

D-ref-1's preset (`sweep.sh:sweep_bg`, 48 rows: 3 clips x rc{-1,2} x gop{-1,4} x
cabac{0,1} x threads{1,4}, 72-80 frames each) calibrated per S55 before any of its
verdicts were used.

**Entry (probe 1 — `eprintln!` at each entry, counted in `compare.sh`'s captured
stderr; reverted):**

| row | `BackgroundDetection` | of those `bDetectFlag=true` | `WelsMdBackgroundMbEnc` | `SvcMdSCDMbEnc` |
|---|---:|---:|---:|---:|
| Static 152x100 rc=-1 gop=-1 t=1 | 80 | 78 | 4524 | 0 |
| Static 152x100 rc=2 gop=-1 t=1 | 80 | 78 | 4754 | 0 |
| Static 152x100 rc=2 gop=4 t=4 | 80 | 40 | 2417 | 0 |
| VT2people 160x96 rc=2 gop=-1 t=1 | 75 | 73 | 1159 | 0 |
| VT2people 320x192 rc=2 gop=-1 t=4 | 72 | 70 | 5771 | 0 |
| **Static 152x100 bg=0 (dark control)** | **0** | **0** | **0** | **0** |

The control is the load-bearing row: it is the same configuration with the new
argument off, and it reproduces exactly the zero F117/T9.B27 measured. The axis is
what lights the family, and nothing else in the preset does.

**Teeth (probe 2 — one planted sample, reverted).** `*pDstLuma =
(*pDstLuma).wrapping_add(1)` immediately after `WelsMdBackgroundMbEnc`'s `McLuma_c`
failed **32 of 48** `bg` rows, while the `sl` preset (`bg` unset) stayed 12/12 — so
the fault is reachable only through the new axis, which is what a planted fault has to
show. Character of the failure: byte counts moved by 3-4272 bytes, e.g. VT2people
320x192 rc=2 gop=-1 cabac=1 C++ 1031358 / Rust 1032946.

**The 16 rows that did not fail are a finding, not a rounding.** Every
`Static_152_100` row passed — and still passed when the planted fault was escalated
from `+1` to `+128`. On that clip the background prediction is genuinely **byte-inert**:
its background macroblocks all take `bSkipMbFlag`, so they code as `MB_TYPE_BACKGROUND`
P_SKIP with no residual, and `WelsMdInterJudgeBGDPskip`'s decision inputs
(`pVaaBackgroundMbFlag`, `CheckChromaCost`) come from the analyzer's *source-domain*
VAA planes rather than from the reconstruction, so the corrupted prediction never
feeds back into a decision either. **4524 entries per row and zero byte sensitivity in
the same row** is the exact shape S55 exists to catch: an entry count is evidence that
a path *runs*, never that a gate can *see* it. The MD family's teeth in this preset are
the two `CiscoVT2people` clips' 32 rows, and the report should say 32, not 48.

**The Static rows are still worth their runtime**, for a different consumer. A planted
one-sample fault in `VaaBackgroundMbDataUpdate`'s luma copy (F117's site, session C's
to convert) fails **all three clips** — Static 102666/103615, VT2people-160x96
347582/351854. So the three sites session C inherits are refereed by the whole preset
including Static, where the MD sites are not. Two sites in one function body, one
gate, two different answers to "can it see me".

Cost, measured rather than estimated: 48 rows, **6.2 s release / 8.5 s debug** standalone,
inside a full eight-preset sweep that runs 38 s release / 45 s debug at the session gate.
D-ref-1's brief priced it at "~1 s on a ~40 s sweep" — the real figure is about eight
times that, and about a fifth of the sweep rather than a fortieth. It is still cheap
enough that the preset belongs on the exit list, but a future session pricing another
axis against that sentence would be reading a number nobody measured. Row totals both
ways, at this commit: `st mt def sl ltr ps dl` = **535**, `+ bg` = **583**.

## F127 — `gates.sh`'s sweep row count had been wrong since `dl` landed: 505 quoted, 535 measured

`sweep_gate`'s comment claimed the exit list ran "369 -> 505 configurations per
profile". The findings file quotes **535** in three places (`:599`, `:808`, `:897`) and
is right. Measured directly at `0bf9e0c9` by running the list and reading the tally
rather than by re-adding the presets:

```
RUST_ENC_PROFILE=release bash rust/tools/diffharness/sweep.sh st mt def sl ltr ps dl bg
PASS=583 FAIL=0
```

583 with `bg`'s 48, so **535** without — the number the prose had all along. The
comment is corrected in place with its own provenance line. Two documents disagreed
about a gate's size for a whole phase and nothing caught it, because nothing reads the
tally *as a number*: `sweep_gate` corroborates `PASS=n FAIL=n` against the exit code
(`:231-237`) and never against an expected count. That is a deliberate choice — pinning
the row count would make every new preset a gate edit — but it means the only defence
against a stale quote is quoting the measurement, which is what the corrected comment
now does.

## F128 — the SCD retag is reclassification, and the two censuses disagree about it on purpose

Step 4 moved eight bodies from `port-raw(Phase 9)` / `cursor` to
`SCREEN_CONTENT(dormant)`: `SvcMdSCDMbEnc`, `CalUVSadCost`, `JudgeStaticSkip`,
`JudgeScrollSkip`, `MdInterSCDPskipProcess`, `SetBlockStaticIdcToMd`,
`WelsMdInterJudgeSCDPskip` (`svc_mode_decision.rs`) and `LineFullSearch_c`
(`svc_motion_estimate.rs`). Between them they own **7** of the 9 screen-content census
sites; the other two (`WelsMotionEstimateSearchStatic`, `..Scrolled`) were already
carrying the tag, which is what made F121 notice the family in the first place.

**Nothing was converted and no unsafe code was removed.** The tag census falls
**696 → 688** (`port-raw` 638 → 631, `cursor` 58 → 57) and
`SCREEN_CONTENT(dormant)` rises 8 → 16 — the same eight items, re-owned. A report that
adds this −8 to B4's conversion count is counting one session's work twice.

**The plane census does not move at all, and that is correct.**
`phase9_plane_callers.py` classifies a site by *operand shape* — `safe-now` means every
operand is `src`/`ref`/`cache`/`local`, i.e. convertible — and it has no notion of
reachability. The nine SCD sites remain `safe-now` after the retag because they still
*are* convertible; S57 says they must not be, which is a different question and a
different instrument's. So the honest pair of numbers at B4's close is
**`safe-now` 25 → 20** (five conversions, F126's referee behind them) with **9 of the
remaining 20 tagged dormant**, and a reader who wants Phase 9's real backlog must
subtract by hand. Two censuses that disagree are not a defect here; a session that
quotes only the flattering one would be.

`WelsMdInterJudgeSCDPskipFalse` and `WelsInitSCDPskipFunc` keep their Phase 9 tags:
the `False` arm is the slot camera content actually runs, and the installer runs on
every configuration.

## F129 — D-dead-2's closure is bigger than F122 listed, and the reason nothing found it is that the compiler cannot

Re-derived with the deletion (S18's F122 clause) rather than from F122's list. What
went, encoder-side only:

| item | where | why it is dead |
|---|---|---|
| `SUB_MB_TYPE_4x4`/`_8x4`/`_4x8` refinement arms | `svc_base_layer_md.rs:1815-1952` | F122's probe: 0 entries, 3 configurations |
| `UpdateP4x4MotionInfo`/`UpdateP8x4MotionInfo`/`UpdateP4x8MotionInfo` | `svc_mode_decision.rs` | 3 call sites, all inside those arms |
| `UpdateP4x4Motion2Cache`/`UpdateP8x4Motion2Cache`/`UpdateP4x8Motion2Cache` | `svc_mode_decision.rs` | **0 call sites** — they survived on an unused `use` line |
| `sMe4x4`/`sMe8x4`/`sMe4x8` | `md.rs:191-193` | 32 `SWelsME`, 3072 of `SWelsMD`'s 4000 bytes |
| **`SWelsMeContainers`** | `svc_mode_decision.rs:175` | **a whole second declaration of `SWelsMD_sMe`, 0 references anywhere** |
| `g_kiPixStrideIdx4x4` | `svc_base_layer_md.rs:910` | its only reader was the `_4x4` arm |
| `SUB_MB_TYPE_8x4`/`_4x8`/`_4x4` consts (u8) | `svc_mode_decision.rs:125-127` | no encoder path assigns them |
| the same three consts (u32) + their `sub_mb_type` and mvd arms | `svc_set_mb_syn_cavlc.rs`, `svc_set_mb_syn_cabac.rs` | 17 mentions, exactly |

**394 lines deleted, 114 added, net −280**, and `assert_size!(SWelsMD, 4000)` re-pinned
to **928**.

**Two items F122 did not have.** `SWelsMeContainers` is the one worth stopping on: a
`#[repr(C)] #[derive(Copy, Clone, Default)]` struct field-identical to `SWelsMD_sMe`,
never constructed, never named in a signature, never re-exported. `UpdateP*Motion2Cache`
is the same shape one level down — three public functions whose only trace in the crate
was an unused import.

**Why neither the compiler nor a warning found them, which is the generalisable part.**
Two independent blindfolds, either of which alone would be enough:

1. Every encoder module carries `#![allow(non_snake_case, non_upper_case_globals,
   non_camel_case_types, **dead_code**)]` — a blanket the naming lints needed, with
   `dead_code` riding along.
2. **Even with that allow removed, rustc reports none of these.** Measured: the allow
   was stripped from every module in `src/` and the crate rebuilt — 14 dead items
   crate-wide, not one of them in this closure. Every item here is `pub` in a `pub mod`
   of a library crate, so rustc considers it reachable by definition. A line-by-line
   port makes almost everything `pub`; dead-code analysis is structurally blind to it.

So a straggler sweep in this crate **cannot** be a compiler pass. It is a grep for each
symbol's reference count with definitions excluded, and that is how this closure was
derived. Recommend S18 say so in those words — "run the compiler's dead-code pass" is
advice that returns 14 items and a false sense of completeness.

**The deleted syntax-writer arms are `unreachable!`, not a silent fall-through.** Both
writers previously ended their sub-8x8 chains in `_ => {}` / a dropped `else`. Keeping
that shape would mean an unexpected `uiSubMbType` writes *nothing* — which in CAVLC
desynchronises the rest of the slice and in CABAC desynchronises the arithmetic coder,
i.e. silent corruption of every macroblock after it. A panic is the better failure, and
it is also the marker a future sub-8x8 re-port will trip over immediately.

**Not touched: the decoder.** `SUB_MB_TYPE_4x4`/`_8x4`/`_4x8` keep 50 references across
`decode_slice.rs`, `parse_mb_syn_cavlc.rs`, `parse_mb_syn_cabac.rs`, `mv_pred.rs` and
`error_concealment.rs`. A decoder must parse any conforming stream whatever partitions
its encoder chose; only the encoder's own inability to produce them is at issue here.

Gates: **583/583 in both profiles** — the full exit list including the 48 new `bg` rows.
A deletion inside the two bitstream writers gets the byte gate at family scale, not the
commit gate's word for it.

## F130 — D-cov-1's inventory: three mc tests die, not two, and `common/mc.rs` needs three allows, not two

Two corrections to B4's brief, both found by executing it.

**The tests.** The brief says "**both** surviving tests together —
`mc_shims_stay_inside_the_spans_they_declare` (`tests/kernels_differential_phase2.rs:251`)
and the Phase 4a dispatch assert-map **inside `mc.rs`'s test module** (F124's count)".
Two errors in one sentence:

* The dispatch assert-map is `mc_table_slots_match_the_direct_calls` and it is in
  `tests/kernels_differential_phase2.rs:2343`, not in `mc.rs`. F124 has it right; the
  brief mislocated it while citing F124 for the count.
* There is a **third**: `test_mc_horiz_and_vert_luma_aliases`, in `mc.rs`'s own test
  module, proving `McHorizLuma_c == McHorVer20_c` and `McVertLuma_c == McHorVer02_c`.
  All four of those are among the 26 deleted, so it fails to compile the moment they
  go — which is how it was found. It is not rewritten against the safe kernels: the
  aliasing it pinned is a property of the C header, and `mc_hor_ver20`/`mc_hor_ver02`
  have no aliases to disagree with.

Two `InitMcFunc` tests **stay** and were not on anyone's list either:
`init_mc_func_ignores_the_cpu_flag` and
`mc_table_is_all_none_before_init_and_all_some_after`. Both are still real properties
of the installer, and `encoder_context.rs:2464`'s construction assertion leans on the
second.

**The allows.** The brief says `common/mc.rs` reaches `#![deny(unsafe_code)]` "with
exactly the two dormant-tagged allows". It needs **three**. `McLuma_c` and `McChroma_c`
survive as the brief says, but both go through `shim_wh` — the `unsafe fn` that
materialises the caller's pointer pair as the two slices the safe kernels take, sized
to the declared reach. It is not screen-content code, but it has exactly their lifetime;
it is tagged `SCREEN_CONTENT(dormant)` with that said in the comment, and all three
retire together in Phase 10.

**One item nobody's list had at all**: `PMcChromaWidthExtFunc`,
`PWelsSampleWidthAveragingFunc` and `PWelsMcWidthHeightFunc` — three `unsafe extern "C"
fn` type aliases with zero references anywhere in the crate. `mc.h`'s width-specialised
dispatch shapes, which this port never dispatched through. `deny(unsafe_code)` does not
fire on a type alias, so they would have survived the flip silently and left three raw
spellings in a module whose whole point is no longer having any. Deleted; found by
grepping every `unsafe` in the file after the flip rather than by trusting the flip.

**What `SMcFunc` is now.** The six slots hold the safe kernels — `mc_luma`, `mc_chroma`,
`mc_hor_ver20`/`_02`/`_22`, `pixel_avg` — and the three slot *types* name those
signatures. `assert_size!(SMcFunc, 48)` holds unchanged: six `Option<fn>` at 8 bytes by
null-pointer optimisation, checked rather than assumed. Worth stating plainly:
**nothing in `src/` reads these slots.** Phase 4a de-virtualized both codecs' motion
compensation and `BaseMC` names the kernels directly (`decode_slice.rs:1081`); the
readers are `encoder_context.rs:2464`'s `is_none()` assertion and, until this commit,
one test. The table is kept, filled and pinned because it is upstream's
(`codec/common/inc/mc.h:46`) and the ABI guard's — not because anything dispatches
through it. The retype is therefore cheap and the deleted dispatch test genuinely
spent: "slot equals direct call" has become "`mc_luma == mc_luma`", and the mistake it
guarded against is now a type error at `InitMcFunc`.

## F131 — the reconstruction seam is a *picture* view, not a plane view: 11 of the 14 contested sites are not planes at all

Session C's brief, and F107 §3/§4 behind it, scope D-mt-3 as a **plane** seam: "one seam
type over each reconstruction plane", "workers share `&SharedPlane` and write through
`&self`", consumers counted as "32 blocked call sites — 17 copy, 10 idct, 5 intra-pred —
plus deblocking's own walk". Fact 4 of the brief treats the reconstruction picture's
other route as bookkeeping: "there are **18** `layer_dec_pic`/`_mut` uses … every
in-frame one must become a view route or be proven pre-fork/post-join. List them in the
report."

**Listing them is not the work; nine tenths of the work is in that list.** The 18 lines
are 4 imports and **14 call sites**, and only **4** of the 14 reach a plane:

| what it reaches | sites | where |
|---|---:|---|
| `sMvList` | 5 | `svc_mode_decision` 665/1305/2088, `svc_base_layer_md` 988/1506 |
| `pMbSkipSad` | 3 | `svc_base_layer_md` 955 (read), `svc_encode_slice` 2371/2544 (write) |
| `uiRefMbType` | 2 | `svc_encode_slice` 2371/2544, the same two calls |
| `pRefMbQp` | 1 | `svc_mode_decision` 1736 |
| the `sMvList.is_empty()` test | 1 | `svc_mode_decision` 1304 |
| **a plane** (`planes()`) | **4** | `svc_base_layer_md` 359, `svc_encode_slice` 1893, `deblocking` 1367/1436 |

Ten of the fourteen reach four **per-macroblock side arrays** of `SPicture` —
`sMvList: Vec<SMVUnitXY>`, `pRefMbQp: Vec<u8>`, `pMbSkipSad: Vec<i32>`,
`uiRefMbType: Vec<u32>`. A plane view cannot carry them, and F107 §3's argument applies
to them *verbatim*: a `&mut Vec<T>` is a `Unique` retag over the whole array however
narrow the write, so "worker *w* stamps only its own macroblocks' entries" is exactly as
inexpressible as "worker *w* writes only its own macroblocks' pixels".

**And it is not a detail at the margin — it is the first thing that fails.** Running the
ignored fork/join probe under Miri *before* writing any code (the measurement this
session opened with, and the one the brief did not ask for) reports:

```
error: Undefined Behavior: Data race detected between (1) retag write on thread
`unnamed-2` and (2) retag write of type `encoder::encoder_context::SRefList` on
thread `unnamed-3` at alloc294067
   --> src/encoder/encoder_context.rs:406  pub fn pic_mut(&mut self, ...)
   1: encoder::svc_encode_slice::layer_dec_pic_mut
   2: encoder::svc_base_layer_md::WelsMdIntraInit  (svc_base_layer_md.rs:359)
```

The race is not on a pixel. It is not even on the picture: it is on `SRefList`, the
**pool**, because `layer_dec_pic_mut` goes through `SRefList::pic_mut(&mut self)` and two
workers take that borrow at once. A seam that covers only the pixel planes leaves every
one of the fourteen sites on that route and cannot move the probe one instruction.

So `RecPicView` (`encoder/rec_view.rs`) carries the three planes **and** the four side
arrays, and its one `unsafe impl Sync` sits on `SharedCells<T>`, the captured
base/length pair all seven are built from. The side arrays converted in T9.C3, the four
plane roots in T9.C4, and `layer_dec_pic`/`layer_dec_pic_mut` are both deleted.

*Two smaller errors in the same brief, recorded so the next one does not inherit them:
it instructs "your findings start at **F130**", and F130 was written by session B4 at the
commit the brief itself was authored on top of (`72fa0c0c`); and it names the fork sites
as "`std::thread::scope` (`:1443`, `:1511`)", which is `EncodeFixedSlicesForked` and
`UpdateMbMapForked` — **`EncodeSizeLimitedSlicesForked` at `:1727` is a third fork and
is missing**, which matters because it is the one the brief's own mid-row probe
suggestion (`SM_SIZELIMITED_SLICE` at threads=2) drives.*

## F132 — the fork contends over **six** shared families, not one, and F107's acceptance was assigned to the wrong session

F107 §4 makes the acceptance of session C "the Miri probe
`fork_join_encodes_a_multi_slice_frame_under_the_aliasing_checker` loses its
`#[cfg_attr(miri, ignore)]` and passes", and the plan's D-mt-3 repeats it. That reads as
if the probe were failing *because of* the reconstruction write. It is not. The probe is
the whole fork's UB detector, and the reconstruction write is one of six things the fork
shares.

Method: fix the family Miri names, re-run, read the next verdict. Six rounds, each
verdict quoted in the commit that answered it.

| # | shared state | shape | verdict | owner |
|---|---|---|---|---|
| 1 | reconstruction picture — 3 planes + `sMvList`/`pRefMbQp`/`pMbSkipSad`/`uiRefMbType` | `&mut SPicture` per worker, per macroblock, via `SRefList::pic_mut` | race on `SRefList` | **session C** — fixed (T9.C3/C4) |
| 2 | `Option<Box<SStrideTables>>` | four read-only lookup accessors taking `&mut self` down to `Vec::as_mut_ptr` | race on the `Option<Box<..>>` | families 4/6 — fixed here (T9.C4): the four accessors go `&self`/`*const`, the four `AllocStrideTables` writers spell `root()` at the site, and two S40 tests split their read and write halves |
| 3 | `SWelsSvcRc::pGomCost` | `&mut Vec<i32>` per macroblock, then `+=` at a **shared** index | write/read race between two workers *inside* `WelsRcMbInfoUpdateGom` | X1–X2 (RC) — **a genuine upstream race**, see F133; made `AtomicI32` here |
| 4 | `SDqLayer::sSliceEncCtx` | `WelsGetNextMbOfSlice` and `WelsMbToSliceIdc` borrowing the whole `SSliceCtx` mutably to read it | race on `SSliceCtx` | families 4/6 — fixed here (T9.C5), two lines |
| 5 | `SMB` | deblocking reads a **neighbour's** `uiSliceIdc` (`pCurMb.offset(-iMbStride)`) while the worker encoding that neighbour holds `&mut SMB` | retag-write vs non-atomic read | **session E** — open (F112/F114b, the 31 neighbour-bound `*mut SMB`) |
| 6 | `SSliceCtx::pOverallMbMap` | `AddSliceBoundary` **rewrites** the map in-fork through `&mut Vec<u16>`; `WelsGetNextMbOfSlice` reads `map[mb+1]`, which at a partition's last macroblock is the next partition's | retag-read vs retag-write | the slice structures (D/E) — open |

Rounds 1–4 are closed; the two remaining are the two probes' current verdicts —
row-aligned probe stops at (5), mid-row probe at (6), because the size-limited mode is
the only one that rewrites the map.

**Three of the six are not session C's family by the charter's own §8**, and two of them
(3 and 6) are real races rather than port-introduced over-claims. So the acceptance as
written cannot be met by the session that owns the reconstruction write, and the honest
form of it is narrower: *the seam's own data-race probes pass, and the encoder probes'
remaining verdicts are named*. Both hold at this session's close.

What the enumeration is worth beyond this session: it is the **complete** list of what
`gates.sh session`'s Miri step will report as families 4–6 land, in the order it will
report them, with the owner of each. Rounds 2 and 4 also say something about cost —
neither needed a design, and round 4 was literally two characters twice
(`&mut` -> `&`).

## F133 — `pGomCost` is a data race in upstream OpenH264, on storage nothing ever reads

Round 3 of F132, and it is worth its own entry because the defect is upstream's, not the
port's.

`RcInitGomParameters` sets `iComplexityIndexSlice = 0` for **every** slice
(`ratectl.cpp:665`, and the port at `rc.rs:1548`). `WelsRcMbInfoUpdateGom` then does

```c
pWelsSvcRc->pGomCost[kiComplexityIndex] += iCostLuma;      // ratectl.cpp:1273
```

per macroblock, from every slice's thread, with `kiComplexityIndex` that per-slice
counter. So slice 0's GOM *k* and slice 1's GOM *k* are **one entry**, and the `+=` is a
concurrent non-atomic read-modify-write. `pWelsSvcRc` is per dependency layer, shared by
every slice of the layer; nothing in the C++ synchronises it. Miri:

```
Data race detected between (1) non-atomic write on thread `unnamed-2` and
(2) non-atomic read on thread `unnamed-3`
   1: encoder::rc::WelsRcMbInfoUpdateGom
```

both threads in the same function, at the same entry.

**It is invisible because nothing reads the array.** Exhaustively, in the C++:
`rc.h:191` (the field), `ratectl.cpp:79` (carve), `:90` (null on failure), `:669`
(memset), `:1273` (this `+=`). Five references, not one of them a read. The port had
exactly five with the same property. So a lost update cannot reach a bit of output —
which is why the `mt` sweep has never seen it and why it survived to Phase 9.

Fixed here in the minimal way: the element is `AtomicI32` and the accumulate is
`fetch_add(_, Relaxed)`. Defined where the C++ is not, identical wherever it is
observable, and `assert_size!(SWelsSvcRc, 440)` does not move because that is the same
three words. Two things went with it: `rc_gom_cost` (S18 — its last production caller
was that one statement) and `SWelsSvcRc`'s `Clone` derive, which was needed only by two
`vec![default(); n]` constructions and would have been a trap next to a captured base.

**The deletion is the better answer and it is a ruling, not a session's call.** Dead
storage in both codebases, five references each; deleting the field and the statement
would be provably output-neutral. It is left in place because F115 set the precedent that
"dead in the port *and* dead upstream" is the steward's call, and because the atomic
costs nothing while the question is open.

## F134 — `SPicData.pDecMb` was `pCsMb` under a second name, and the second name was the last per-macroblock `&mut SPicture`

`WelsMdIntraInit` stamped two triples of plane cursors per macroblock:

```rust
iStrideY = (*pCurLayer).iCsStride[0];                            // from the layer
(*pMbCache).SPicData.pCsMb[0] = (*pCurLayer).pCsData[0].offset(iOffsetY);
…
let pDecPic = layer_dec_pic_mut(pCurLayer).expect(..).planes();   // from the pool
iStrideY = pDecPic.iLineSize[0];
(*pMbCache).SPicData.pDecMb[0] = pDecPic.pData[0].offset(iOffsetY);
```

`WelsInitCurrentLayer` fills `pCsData`/`iCsStride` from that same `planes()` call, so the
two triples are one address computed twice — and the second computation is a
whole-picture `&mut` retag, per macroblock, inside the fork. It is the site Miri named
first (F131).

Proved before deleting: `debug_assert_eq!(pDecMb[i], pCsMb[i])` in **both** branches of
the stamp, carried through a whole `gates.sh family` — 583 configurations x 2 profiles —
without firing. Calibrated the assertion by planting `.wrapping_add(1)` on one side,
which aborted (exit 134) on every row of the first preset, so `debug_assertions` is live
in the sweep's debug driver and the pass meant something.

`pDecMb`'s only readers were the luma/chroma triple in
`OutputPMbWithoutConstructCsRsNoCopy`; they read `pCsMb` now, the field is gone, and
`assert_size!(SMbCache)` moves 5600 → **5568** (three pointers plus the 8 bytes of
`align(16)` rounding). Two more `planes()` calls in the same class went with it — that
function's stride pair and deblocking's two root triples, all three reading numbers
`WelsInitCurrentLayer` had already stamped on the layer — and with the last of them
**`layer_dec_pic_mut` is deleted**.

The general form, for the sessions that still have picture accessors to retire: *a raw
cursor stamped from the layer and a raw cursor stamped from the pool can be the same
address, and the pool one costs a retag.* Check for the duplicate before designing a
route for it.

## F135 — `svc_encode_mb.rs::WelsRecPskip` is a dead second implementation, and it is three of the census's "blocked 32"

The plane census attributes **6** blocked copy sites to `WelsRecPskip`, more than any
other owner, which is why the brief names it as the one consumer session C must convert
if it splits. Three of the six are in a function with no caller.

```
$ grep -rn 'WelsRecPskip' src/ | grep -v 'pub unsafe'
svc_mode_decision.rs:366    WelsRecPskip(pCurDqLayer, &*ctx_func_list(pEncCtx), …)
svc_mode_decision.rs:685    WelsRecPskip(pCurDqLayer, &*pFunc, …)
svc_mode_decision.rs:2105   WelsRecPskip(pCurDqLayer, &*pFunc, …)
```

All three resolve to `svc_mode_decision.rs`'s `pub unsafe extern "C" fn WelsRecPskip`.
The one in `svc_encode_mb.rs` — same name, same four parameters, same body modulo
`.as_ptr()` spelling instead of array indexing — has none, in `src/`, `tests/` or
`benches/`. Both are `pub` in a `pub mod`, so F129's rule holds: no compiler pass finds
this, only a per-symbol grep.

T9.C7 converted the live one. The dead one is **not** deleted, and the reason is F128's:
deleting it would take three rows off the blocked census with no conversion behind them,
and the whole point of keeping the two censuses apart is that reclassification and
conversion never get summed. So the count reads **29 blocked** after T9.C7 — 26 mine, 3
of which are dead code — and the deletion is a ruling with the evidence attached.

*While in there: the brief's table calls the third intra-pred array `pfGetIChromaPred`;
it is `pfGetChromaPred` (`wels_func_ptr_def.rs:344`). And the ignore on
`slice_multi_threading.rs:1124` that the brief offers to retire "if the seam retires its
reason" is ignored for **cost**, not aliasing — its own comment says "roughly eight times
the work of the fork/join probe" and "the aliasing question this path raises is the
fork/join's, and that probe answers it". The seam cannot retire that reason and
un-ignoring it would put the largest single test in the battery under Miri.*

## F136 — F132's round 6 is **two** fields, not one, and the second is guarded by a mutex only its writer takes

The C2 brief scopes step 0a as "`SSliceCtx::pOverallMbMap` → atomics, 18 mentions", and
gives the acceptance in advance: re-run the mid-row probe under Miri afterwards and it
"must now name the **`&mut SMB` / `uiSliceIdc`** family (round 5, session E's), not
`SSliceCtx`". The 18 mentions were exact. The acceptance was not met by them.

With `pOverallMbMap` atomic and nothing else changed, the probe still stopped inside
`SSliceCtx`:

```
error: Undefined Behavior: Data race detected between (1) retag read on thread
`unnamed-3` and (2) non-atomic write on thread `unnamed-2` at alloc284162+0x15c
   (2) svc_encode_slice.rs:3266   (*pSliceCtx).iSliceNumInFrame += 1;
   (1) svc_encode_slice.rs:1997   let map: &[AtomicU16] = &pSliceSeg.pOverallMbMap;
```

The offset names the field with no ambiguity. Measured at this commit:

```
SDqLayer.sSliceEncCtx     320  = 0x140
SSliceCtx.iSliceNumInFrame 28  = 0x01c        0x140 + 0x01c = 0x15c
SSliceCtx.pOverallMbMap     0
```

so the racing byte is `iSliceNumInFrame`, and the read that reaches it is not the
`pOverallMbMap` borrow Miri's span points at but the **whole-`SSliceCtx` shared retag**
four lines up (`svc_encode_slice.rs:1982`, `let pSliceSeg = &(*pCurDq).sSliceEncCtx;`) —
the retag T9.C4 already narrowed from `&mut` to `&`, which was enough to stop it racing
against other readers and is not enough to stop it racing against a writer. Under a
whole-struct retag **the fork contends with the struct, not with a field**, so a
field-at-a-time conversion keeps producing new verdicts until every field written inside
the fork is atomic.

**The good news is that the enumeration terminates immediately.** Every write to an
`SSliceCtx` scalar, by grep over `src/encoder`:

```
encoder_ext.rs:2956          UpdateSlicepEncCtxWithPartition   setup
slice_multi_threading.rs:1095 (a unit test)                    setup
svc_enc_slice_segment.rs:546-598  InitSliceSegment             setup
svc_enc_slice_segment.rs:617-623  UninitSliceSegment           teardown
svc_encode_slice.rs:3266     DynSlcJudgeSliceBoundaryStepBack  ** inside the fork **
```

One of thirty is in the fork. So round 6 is `pOverallMbMap` *and* `iSliceNumInFrame`, and
with both atomic the probe advances exactly as the brief predicted:

```
error: Data race between (1) non-atomic read on thread `unnamed-3`
                     and (2) retag write of type `encoder::md::SMB` on `unnamed-2`
   (2) svc_encode_slice.rs:1464  UpdateMbNeighbor(pCurDq, &mut *pMb, ..)
   (1) deblocking.rs:1190        (*pCurMb).uiSliceIdc
                                   == (*pCurMb.offset(-iMbStride)).uiSliceIdc
```

Round 5, F112/F114b's family, session E's. **Both encoder fork/join probes now stop on
the same thing**, where before they stopped on two different families — which is the
useful part for whoever schedules E: one conversion un-ignores two probes, not one.

### Why the mutex did not save it, and why it still has to stay

`iSliceNumInFrame`'s increment is not unguarded. F69 restored `mutexSliceNumUpdate`
around it precisely because the raw translation had dropped it, and the port holds it
across `AddSliceBoundary` *and* the `++`, exactly as `svc_encode_slice.cpp:1776-1791`
does. The lock is real and it is upstream's.

It does not help, because **no reader takes it.** `WelsGetNextMbOfSlice` runs per
macroblock on every worker and reborrows the whole `SSliceCtx` without any lock at all;
`WelsMbToSliceIdc`, `NeedDynamicAdjust` and `ReOrderSliceInLayer` likewise. A mutex only
one side of a conflict acquires is not synchronisation — it orders writers against each
other and says nothing about readers. That is F133's shape a second time (`pGomCost`: a
race nothing could observe) with one difference worth flagging: here the value *is* read,
by `ReOrderSliceInLayer`'s `iEncodeSliceNum != iSliceNumInFrame` test, so this one is not
write-only storage and the atomic is load-bearing rather than hygienic.

**The mutex is therefore not redundant and was not removed.** It brackets the slice-map
rewrite *with* the counter increment; an atomic `fetch_add` makes the counter's own
accesses well defined and cannot make that pair indivisible. Both are needed, and a later
session that reads "the field is atomic now" as licence to drop the lock would reopen F3.

*Method note, since the brief invited it: the brief's count was right and its acceptance
was the thing that caught the gap. A step specified as "convert these 18 sites" would
have been reported done and green; a step specified as "convert these 18 sites, and the
probe must then say X" was not. Write the next brief's steps with the instrument's
expected reading attached — it is the only part of step 0a that failed, and the only part
that could have.*

## F137 — the plane census could not see `pfIDctI16x16Dc`, and had not been able to since it was written

The C2 brief opens with a rule — "re-run before quoting, and trust the tree over this
document" — and applies it to the brief's own numbers. It does not extend it to the
instrument the numbers come from. It should.

`WelsEncRecI16x16Y` has three mutually exclusive reconstruction branches, chosen by how
much residual survived quantisation:

```rust
if uiNoneZeroCountMbAc > 0 {            // four pfIDctFourT4 quadrants — census: 4 rows
    ...
} else if uiCountI16x16Dc > 0 {         // pfIDctI16x16Dc              — census: NOTHING
    func(pPred, kiRecStride, pBestPred, 16, aDctT4Dc.as_mut_ptr());
} else if let Some(func) = ... {        // pfCopy16x16Aligned          — census: 1 row
    func(pPred, kiRecStride, pBestPred, 16);
}
```

All three write `pPred` — `SPicData.pCsMb[0]`, a raw cursor into the reconstruction luma
plane — from `pBestPred`, the `sMemPredMb` arena, at the same two strides. The census
listed the first branch and the third and was silent about the second, because

```
$ grep -n 'pfIDctI16x16Dc\|I16x16Dc' tools/phase9_plane_callers.py
$                     # (no output)
```

Neither the `pfIDctI16x16Dc` slot nor the `WelsIDctRecI16x16Dc_c` kernel installed into
it appears in either of the tool's two classification tables, so calls through that slot
fall through as unclassified rather than as `blocked`. The slot has its own Rust type
(`PIDctI16x16DcFunc`, `svc_encode_mb.rs:310`) distinct from `PIDctFunc`, which is
probably how it was missed: whoever added the idct family to the census added the type's
two members and not the singleton beside it.

**The correction, and the accounting.** With the slot and its kernel added:

| | blocked | mine | Phase 10's (SCD) |
|---|---:|---:|---:|
| session C's close, as the brief states it | 29 | 26 | 3 |
| after F135's deletion (3 rows, no conversion) | 26 | 23 | 3 |
| after this correction (+1 row, no conversion) | **27** | **24** | 3 |

F128 keeps *reclassification* and *conversion* from being summed. This is a third
category and needs the same discipline: a **census correction** is neither. Three rows
left because code was deleted, one arrived because the instrument was fixed, and not one
of the four is a raw site converted to the seam. The conversions are counted separately
and they are 11 (idct) + 8 (copy) + 5 (intra-pred).

**What the site itself needed was nothing special.** It is `idct_rec_i16x16_dc_to_view`,
converted in T9.C2d beside its four siblings, and the sweep does referee it — the DC-only
branch is reached whenever an I16x16 macroblock quantises to DC alone, which is common in
flat content and is why the `st` preset's Static clip exercises it. The defect was never
in the code; it was that the number this session budgeted against was one too small, and
nothing in the pipeline would have said so.

*Generalisation for the remaining sessions: the census's tables are hand-maintained lists
of slot names, and a slot with a singleton type is exactly the shape that gets left out.
Before trusting a family's count, grep the tool for every `pf*` name in that family's C++
header — the census cannot report what it was never told to look for, and a missing row
is indistinguishable from a converted one.*

## F138 — the intra-pred family converted in one step because it had exactly three raw helpers, and it took two probes' worth of coverage with it

`encoder/get_intra_predictor.rs` looked like the session's expensive step: twenty-six
`unsafe extern "C" fn(pPred, pRef, kiStride)` shims, plus two more imported from
`common/intra_pred_common.rs`, across three dispatch tables and twenty-eight modes. It
was the cheapest, and the reason is a *shape* rather than luck.

**Every shim was one line, and every line called one of three helpers.**

```rust
unsafe { i4x4_luma_pred_v(pred::<16>(pPred), top_row::<4>(pRef, kiStride)) }
unsafe { i4x4_luma_pred_h(pred::<16>(pPred), &reference(pRef, kiStride, REACH_I4X4_LEFT)) }
unsafe { i4x4_luma_pred_dc_na(pred::<16>(pPred)) }
```

`pred` built the destination array, `top_row` the row above, `reference` a `PlaneCursor`
over exactly the mode's reach. Nothing else in the file touched a pointer. So the whole
family's raw surface was **three functions**, and the conversion was: give the kernels a
way to read a reference that is not a slice, rewrite twenty-eight one-liners
mechanically, delete the three.

The trait is `safe::plane::RefSamples`, and it is small because the kernels are
disciplined — measured over both files, every read of the reference is one of two calls:

```
$ grep -o 'reference\.\w*' src/encoder/get_intra_predictor.rs src/common/intra_pred_common.rs \
    | sort | uniq -c
  45 reference.at
   6 reference.row
```

Two methods, two impls (`PlaneCursor` and the seam's `RecCursor`), static dispatch, no
copy, and one set of kernels still serving both.

### What it cost, which is the part worth the entry

Deleting the shims deleted the subject of two Miri probes in
`tests/kernels_differential_phase2.rs` —
`encoder_intra_pred_shims_stay_inside_the_spans_they_declare` (with its `probe_intra`
driver and three `safe_*` builders, ~325 lines) and
`i16x16_luma_pred_shims_stay_inside_the_spans_they_declare`. Both probed *hand-written
span contracts*, and there are none left. D-cov-1's rule says name what dies:

| property the probe pinned | what holds it now |
|---|---|
| **span tightness** — reference allocation sized to `ref_span`'s exact claim, so an over-claim was an OOB read Miri reported | `RecCursor` bounds-checks every access against the whole plane allocation: an over-reach panics instead of being UB. The narrower claim — an over-reach that stays inside the plane but outside the mode's availability — was never this probe's job; it is `reach_table_agrees_with_the_availability_tables`', which stands unchanged. `ref_span` and the `REACH_*` constants stay for it. |
| **anchor** — shim output must equal the safe kernel at the contract's geometry, killing an off-by-a-row anchor | Vacuous: the shim *is* the safe kernel. The geometry is pinned by the rewritten unit tests, which read their expectations through the cursor, and end to end by the sweep — a one-column anchor slip planted in `WelsI4x4LumaPredV_c` failed **210 of 210** `st` rows, and the same slip in the moved `common` adapter failed 210 of 210. |
| **destination extent** — eight bytes of noise slack each side of the packed block | `&mut [u8; N]`. Writing past it does not compile. |

Two of the three are now *type-enforced* rather than *tested*, which is the trade the
phase is making; the third moved to a test that already existed. The retirement is a
strict improvement, but it is still ~370 lines of Miri probe gone, and the next coverage
audit should find this entry rather than a hole.

### The ratchet, and what it says about metric choice

    raw_ptr      2000 -> 1937    unsafe_fn    728 -> 696
    unsafe_block  366 -> 312     shim          65 ->  37

Compare T9.C2d and T9.C2e — nineteen reconstruction sites converted, **ratchet flat on
every metric**. Those sites called through pointers already bound inside `unsafe fn`
bodies with other raw work; this family's conversion deleted whole `unsafe fn` items.
*The ratchet measures declarations, not call sites*, so a session that converts consumers
reads as zero progress on it and a session that deletes wrappers reads as a landslide.
Both are real work; neither number is the work. For seam sessions the plane census is the
instrument — 27 blocked to 3 across T9.C2d/e/f — and the ratchet is the floor that must
not rise.

### Two tables did not flip, and their reasons differ

* **`PIDctFunc`** — after T9.C2d the only raw reader of `pfIDctFourT4` was
  `WelsIDctT4RecOnMb`, whose one caller (`OutputPMbWithoutConstructCsRsNoCopy`) that same
  commit converted. The helper is now dead, and with it dead the three `pfIDct*` slots are
  *installed, asserted-installed, and never called* — `pGomCost`'s shape (F133) in a
  dispatch table. Flipping a table nothing reads is churn; deleting one is a ruling. Left
  as measured, for the steward.
* **`PCopyFunc`** — genuinely blocked, and not by this session. Four raw readers remain:
  `VaaBackgroundMbDataUpdate`'s two (F117's, still open, source-picture copies) and
  `SvcMdSCDMbEnc`'s two (Phase 10's, `SCREEN_CONTENT(dormant)`). `common/copy_mb.rs`
  cannot reach `#![deny(unsafe_code)]` until both go, because its `copy_shim` is what the
  seven `WelsCopy*_c` wrappers are built from. **`copy_mb.rs` reaching `deny` is therefore
  F117's exit criterion as much as Phase 10's** — worth pairing them in whichever session
  takes F117.

## F139 — deblocking's encoder half converts cleanly; the two `deny`s the brief promised are both blocked, and by different sessions

The C2 brief's step 6 is one sentence with three clauses: "the per-MB walk uses view
cursors; `deblocking_common.rs`'s edge filters gain the `&self`-write flavour; both files
reach `deny`." The first two landed. Neither `deny` can, and it is worth separating why,
because the two reasons belong to two different owners.

### First, the brief's counts, which are wrong in both directions

```
encoder/deblocking.rs        brief: "26 unsafe items, 21 tags"
                             tree:  19 unsafe fn + 1 unsafe block = 20 items, 21 tags,
                                    already under #![deny(unsafe_code)] with 21 allows
common/deblocking_common.rs  brief: "19 unsafe fn, un-denied"
                             tree:  13 unsafe fn (12 edge shims + WelsNonZeroCount_c)
                                    + 15 unsafe blocks; un-denied is right
```

Three `grep`-visible `unsafe extern "C" fn` lines in `encoder/deblocking.rs` are `pub
type` aliases, not definitions, which is most likely where 26 came from. The tag count
(21) is exact.

### What converted, and why it was cheap for the same reason F138's family was

`common/deblocking_common.rs`'s twelve shims are one line each over **four** safe kernels
(`deblock_luma_lt4`/`_eq4`, `deblock_chroma_lt4`/`_eq4`), and those kernels touch their
plane through exactly two calls:

```
$ grep -oE 'pix\.\w+' src/common/deblocking_common.rs | sort | uniq -c
  22 pix.at
  18 pix.set
```

So `safe::plane::PlaneSamples` is `RefSamples` plus `set`, two impls
(`PlaneCursorMut`, `RecCursor`), static dispatch — and the *decoder*, which already calls
these kernels directly with `planeY.cursor_mut(..)`, is untouched by the change. One
kernel set, two storages, no copy. That is the second time in this session that a family
turned out to be convertible in one pass because its raw surface was a handful of
helpers rather than a habit spread through the bodies.

The encoder's eight `FilteringEdge*` dispatchers are safe now, and
`SDeblockingFilter::pCsData: [*mut u8; 3]` — three raw plane roots the frame and slice
drivers re-advanced per macroblock — is deleted in favour of `mb_cursors`, which derives
the same three addresses from the seam view and the macroblock's own `(iMbX, iMbY)`. The
arithmetic moved one level down and can no longer drift out of step with the loop that
used to maintain it.

### Why `encoder/deblocking.rs` cannot reach `deny` — session E's

13 `#[allow(unsafe_code)]` remain, down from 21. Every one is on a function whose
*parameters* are raw: `*mut SMB` (`DeblockingInterMb`, `DeblockingIntraMb`,
`FilteringEdgeLumaHV`, `FilteringEdgeChromaHV`, `DeblockingMbAvcbase`,
`DeblockingBSCalc_c`), `*mut SDqLayer`, `*mut sWelsEncCtx`, `*mut DeblockingFunc`. The
`*mut SMB` ones are F112/F114b's family and F132's **round 5** — the same neighbour-bound
`SMB` pointers whose `uiSliceIdc` reads both fork/join probes now stop on (F136). This
file's `deny` is therefore not a deblocking task at all; it falls out of session E's
`&mut SMB` work, and should be listed as one of E's exit criteria rather than left as an
orphan.

### Why `common/deblocking_common.rs` cannot reach `deny` — the decoder's

Its twelve shims have no encoder reader left, but they are not the encoder's to delete:
`decoder/deblocking.rs:208` re-exports the module wholesale, `decoder_core.rs:2142` calls
its `DeblockingInit`, and the decoder's `DeblockingFunc` reads every slot. Deleting them
means flipping the decoder's dispatch table, which is a decoder session's work and
outside an encoder session's lane.

### The third write-only table this session found

With the eight dispatchers calling their kernels directly (F118 — `DeblockingInit`
installs unconditionally and nothing rewrites), the encoder's own `DeblockingFunc` now
has **8 of 10 slots installed and never read**: the four `pfLumaDeblocking*` and four
`pfChromaDeblocking*`. `pfDeblockingBSCalc` and `pfDeblockingFilterSlice` are still read
and stay.

That makes three in one session — `pGomCost` (F133), the three `pfIDct*` slots (F138),
and now these eight. **The shape is worth naming**: converting a consumer to call its
kernel directly does not retire the slot, it silently demotes it to write-only storage,
and nothing in the build or the gates notices. A `pf*` slot that is assigned, asserted
`is_some()`, and never invoked is indistinguishable from a live one by every instrument
this project has except a per-symbol grep for the read. Whoever writes the phase-exit
checklist should add one: *for each dispatch table, grep for a read of each slot, not
just an install.* All three are left exactly as measured, under F133's ruling that
write-only storage is not this phase's to delete.

### Calibration

Two faults, planted separately, `st` preset:

```
FilteringEdgeLumaH's tap steps transposed (step_x/step_y swapped)  180 of 210 fail
mb_cursors' chroma origin one row down                             208 of 210 fail
reverted                                                           210 of 210 pass
```

Deblocking touches every reconstructed sample of every frame, which is what those
numbers say; it is also why this was the session's highest-risk commit and got a full
`family` gate rather than a `commit` one.

## F140 — the seam costs **2.6x** in Miri wall-time, no gate measures it, and the session close gate no longer fits

The C2 close gate was run three times and never produced a verdict. The first attempt
stalled and was abandoned; the second ran 1.5 hours and was killed by the user; a third,
scoped to single probes, was killed after the numbers below were in hand. **This session
has no green `MIRI_SCOPE=encoder gates.sh session`**, and the reason is not a defect.

### The measurement

Two encode probes, timed at `e1cb5ad9` (this session's first commit, before any seam
consumer landed) and at `41ff57ad` (its last):

| probe | pre-session | HEAD | ratio |
|---|---:|---:|---:|
| `encode_loop_runs_over_a_macroblock_grid_…` | **96.5 s** | **254.3 s** | **2.6x** |
| `encode_loop_runs_over_size_limited_dynamic_slices_…` | **422.9 s** | ≥19 min, killed | **≥2.7x** |

Session C's whole encoder-scoped run was **1012 s**. At 2.6x that is ~44 minutes on a
quiet machine, and this one was not quiet.

### Two wrong diagnoses, and what killed each

Worth recording because both were plausible and both were wrong, and the disproofs were
cheap:

1. **"Cursor construction is the cost."** `SharedPlane::cursor` calls
   `SharedCells::cells`, which is `from_raw_parts` over the *whole plane*; Stacked
   Borrows charges a retag proportional to the range. Several converted sites build
   cursors inside mode loops where they are loop-invariant — `WelsMdIntraChroma` builds
   two per mode, `WelsMdI16x16` one. **Disproved by hoisting them: 254.26 s → 253.93 s**,
   a 0.1% move. Cursor construction is not where the time goes.
2. **"The atomics are the cost."** `fill_mb_map` went from one `fill()` memset to a
   per-entry relaxed store, and `AddSliceBoundary` calls it per slice boundary — which is
   reached only under `SM_SIZELIMITED_SLICE`, exactly the probe that stalled. **Disproved
   by the grid probe**, which has almost no slice boundaries and regressed 2.6% anyway.

The slowdown is *uniform across both probes*, which rules out any single hot spot and
points at the only thing that changed everywhere.

### What it actually is, and why there is nothing to optimise

Every reconstruction access that was a raw pointer read or write is now a bounds-checked
`Cell` index:

```rust
pub fn at(&self, dx: isize, dy: isize) -> u8 {
    self.cells[idx(self.center, dx, dy, self.stride)].get()
}
```

Miri interprets each access individually, so an index-plus-bounds-check costs a small
multiple of a raw deref — and deblocking, idct and copy touch **every reconstructed
sample of every frame**. A ~2.6x interpreter cost for that substitution is unremarkable.

`at`/`set` are already minimal. The only way to make them cheaper is `get_unchecked`,
which reintroduces exactly the unsafe the seam exists to remove. **This is the price of
the safety, not a defect to fix**, and no future session should spend time hunting for a
hot spot here — the two obvious ones are disproved above.

### The gate hole this exposes, which is the actionable part

**A 2.6x regression in the project's principal safety instrument passed every gate, eight
commits running.** `gates.sh` measures byte-identity (583 rows x 2 profiles), test counts,
the unsafe ratchet and the duplicate census. **None of them measures Miri wall-time**, and
the session-level Miri step reports pass/fail only. The cost grew commit by commit and
nothing said so; it surfaced only when a human noticed a gate taking 1.5 hours.

Concretely, for whoever owns the gates:

* **record the Miri wall-time per session close** the way the sweep's is recorded, and
  fail or warn on a ratio against the previous session rather than on an absolute. The
  charter already quotes these numbers in prose (894 s at D, 978 s at B4, 1012 s at C) —
  they are being *read* as a series and should be *checked* as one.
* **the two encode probes are the whole cost**: 96.5 + 422.9 = 519 s of session C's 1012 s
  before this session, and ~1100 s of ~2600 s after. Any re-scoping decision is a decision
  about these two tests and nothing else.

### The decision this leaves, which is the steward's

Phase 9 has more seam work ahead (session E's `&mut SMB` family, F117, Phase 10), and each
conversion of this kind will add interpreter cost on the same curve. Three options, none
of them free:

1. **Accept a ~45-minute session close.** Honest, and it gets worse with every session.
2. **Move the two encode probes from `session` to `exit`.** Cheap to do; the cost is that
   an aliasing regression inside the encode loop would then be caught at phase exit rather
   than at session close — and F114 is the precedent for that mattering, since the first
   encoder-scoped Miri run over a finished session is exactly what found two UB the byte
   gates had passed.
3. **Shrink the probes' drive** — fewer frames, smaller grids. A coverage trade, D-cov-1's
   rules apply, and the mid-row probe's boundary assertions constrain how small it can go.

Not taken here: it is a gate-policy decision with a coverage cost, and this session had no
mandate for it.

## F141 — D-gate-5 counted two full-encode probes where the tree has three, and both of its numbers were arithmetically unreachable; D-gate-6 replaced the arithmetic

Session E's step 0 executed D-gate-5 as ruled and measured it as it went. Three of the
ruling's quantities did not survive contact:

1. **"The two full-encode Miri probes" are three.** The CAVLC/fine-MD probe
   (`encode_loop_runs_with_cavlc_and_fine_mode_decision_…`) drives the same encode loop
   through the same seam as the grid probe, and measured **216.5 s** at the session's
   start against the grid probe's 258.5 s — the same cost class, unnamed by F140, whose
   "the two encode probes are the whole cost" undercounted by one. It was shrunk with
   the same `cfg!(miri)` pattern in T9.E1, as a correction rather than a silent
   extension.
2. **The "<20 min" target and the "macroblock-count floor" cannot both hold.** The
   realloc chain needs 35 slices, 35 slices need 112x96, and at the seam's interpreter
   cost that geometry costs ~13 min for one test — so a serial session gate under 20
   minutes cannot carry it, shrink or no shrink. The first executed shrink kept the
   floor and honestly missed the budget.
3. **The step-2 acceptance and any per-close budget collide.** The two fork/join probes
   cannot shrink at all: `MIN_NUM_MB_PER_SLICE` floors their geometry at 49 macroblocks
   (112x112) and two frames is the inter-coverage floor. Measured at their first
   GREEN runs (T9.E8): 3356 s and 3449 s — ~57 minutes as a parallel pair; the ~28
   minutes the aborted iterations suggested was half the truth, because the aborts
   all fell in frame 1's opening and the inter frame is the expensive half. No
   15-minute gate carries them.

**D-gate-6 (the user, 2026-08-24, mid-session): the whole session gate is capped at 15
minutes — "even if we need to reduce the amount of tests that we run"** — ruled after
the first post-shrink session run was stopped at ~40 minutes. What meets the cap is not
a smaller test list but a different architecture, T9.E2:

* the Miri step runs as a **background lane in parallel with the native battery** (they
  do not contend: cargo-miri builds under `target/miri` with its own lock), as **one
  compile plus five concurrent shards** — the three encode probes are the three largest
  single tests and get a shard each; the rest splits into an encoder shard and an
  api/common/safe shard;
* the size-limited probe's Miri drive is 48x32x2 (measured 3/3/3 slices — every frame
  still splits and rolls back; the realloc assertion gates to the full drive);
* `MIRI_FULL=1` restores every full drive at `full`/`exit`, and the fork probes are
  skipped **by name** at session scope with the explicit-run command in the D-gate-6
  block — a cost scope in D-gate-2's sense, not a defect skip;
* S61 reports the lane wall; the baseline file documents the regime change.

Validated the same day: the whole session gate, **OVERALL PASS in 542 s** (9:02) — 283
Miri tests across five shards, 583/583 sweeps in both profiles — against ~40 minutes
stopped and an estimated ~55+ serial. A from-scratch close adds ~3 min of Miri compile.
That run is also C/C2's missing session-scope aliasing evidence, under the reduced and
named scope. What session scope no longer sees: the slice-buffer realloc chain under
Miri, and the fork probes except at `full`/`exit` and explicit runs — both restored
exactly there.

## F142 — round 5 closed by an outcome-equality substitution; the brief's value-assert was the race it proposed to measure, and in-fork deblocking exists because validation rewrites idc 0 to 2

Three structural facts the session E brief did not carry, each load-bearing for round 5:

1. **Why deblocking runs inside the fork at all.** The driver requests
   `iLoopFilterDisableIdc = 0`, and `SliceArgumentValidation` rewrites 0 → 2 under
   threading (encoder_ext.rs:1390, upstream encoder_ext.cpp:2061: "not allowed with
   multithreading"). At idc==2 multi-slice the parallel-deblocking flag survives
   (multi-slice + idc==0 would falsify it), `DeblockingFilterSliceAvcbase` is installed
   per frame, and `uiFilterIdc == 1` makes the `uiSliceIdc` guards the live arm. The
   brief's hedge — "the probe may advance to `uiMbType`/QP/MVs next" — is structurally
   closed by the same fact: at idc==2 these guards *refuse* cross-slice edges, so every
   deeper neighbour read (QP averaging, the BS calc) is same-slice-only, which is
   same-worker.
2. **The brief's proof plan measured the wrong thing with the wrong instrument.**
   `debug_assert_eq!(record, map)` "at every deblocking read, carried through family +
   mt": (a) the assert's neighbour-record read *is* the cross-thread race being removed
   — it cannot run under `mt` or Miri at all; (b) the value equality is legitimately
   false cross-partition mid-frame (a stale record holds a previous frame's `q + k'N`
   against the fresh map's `q + kN`), so the assert would fire on a correct tree and
   the brief's fallback — build a seam view — would have triggered on a false alarm.
3. **What is actually true, and how it was proved (T9.E3/T9.E4).** The comparison's
   *outcome* is interleaving-invariant: same-partition neighbours are final with
   record == map (asserted race-free at every record stamp — `UpdateMbNeighbor` plus
   the four `kiSliceIdx` stamps — and at the guards on the post-join frame walk, where
   uiFilterIdc == 0 means single-threaded by construction); cross-partition, every
   value a partition-q record or map entry can ever hold is ≡ q (mod
   `iActiveThreadsNum`) — `UpdateSlicepEncCtxWithPartition` stamps q,
   `AddSliceBoundary` steps by the thread count and rewrites only from the rolled-back
   macroblock up — while the current macroblock's index is ≡ p ≠ q, so both readings
   refuse the edge under every interleaving. Calibration: the guard assert planted +1
   aborted **210 of 210** `st` rows (exit 134) and the grid probe; the write-site
   assert aborted the dynamic probe and both fork probes at mb 0. The flip then made
   the guards read the map (`family` PASS, 583/583 both profiles, bytes unmoved), and
   `grep 'offset(-…).uiSliceIdc'` over `encoder/deblocking.rs` is empty: in-fork
   deblocking touches no other worker's `SMB` record. Where upstream's eager guard
   reads are a benign-by-outcome data race, the port's relaxed load is defined with
   the same observable result — F133's pattern.

One measured aside: the brief's "the fork boundary keeps its raw mint … the model is
the value assert" arithmetic on assert placement also missed that **one guard site
covers all eight reads** — every deblocked macroblock flows through
`DeblockingMbAvcbase` and nothing writes `uiSliceIdc` within a pass.

## F143 — F132's six-family enumeration was bounded by its earliest abort: closing round 5 unmasked three more shared-state families in two probe iterations

F132 presented its six rounds as "the complete list of what the Miri step will report
as families 4–6 land, in the order it will report them." The list was complete only
relative to where the probes aborted: Miri stops at the first UB, so an enumeration
made by fixing-and-re-running sees exactly one frontier at a time, and round 5's
deblocking race — hit in frame 0 — stood in front of everything later in
interpretation order. With round 5 closed (T9.E4), the two probes advanced past frame
0 for the first time and named, across two ~28-minute iterations:

* **Round 7** (fixed-slice probe): `WelsCodePSlice`/`WelsCodePOverDynamicSlice` stamp
  `pfInterMd` into the **shared** function list per slice, from inside the fork — N
  workers writing the same bytes with no ordering, upstream's own shape
  (`svc_encode_slice.cpp:733/750`), F71/F133's benign-same-value class. Fixed by
  F71's pattern: the stamp is loop-invariant across a frame's slices and hoisted to
  `PreprocessSliceCoding`, beside the deblocking-slot install.
* **Round 8** (fixed-slice probe, next iteration): in-fork
  `Vec::as_mut_ptr()`/array-`as_mut_ptr()` mints on **shared** state — the method
  autorefs `&mut`, so every worker retag-writes the shared header per macroblock:
  `pVaaBackgroundMbFlag` (two sites), `sVaaCalcInfo.pSad8x8`, and
  `sSampleDealingFuncs.pfSampleSadRaw` (read-only use behind a `*mut` habit). All
  re-spelled on `addr_of!`, `thread_bs_buffer`'s proven pattern; `GetChromaCost`'s
  parameter went `*const` to match its reads.
* **The mid-row probe's parallel find**, not a race: `pLayerBsInfo` is minted by
  `(*pFbi).sLayerInfo.as_mut_ptr()` — the array method autorefs
  `&mut (*pFbi).sLayerInfo`, so the "raw" was a raw-above-a-Unique, and the
  size-limited branch's sibling `pLbi` write popped it before `SliceLayerInfoUpdate`
  wrote back through it. T5.O8/F70's rule in a new costume: the mint is
  `addr_of_mut!((*pFbi).sLayerInfo).cast()` now, a place projection sharing `pFbi`'s
  provenance, and raw-sibling writes do not pop raw siblings.

The general lesson extends S60's: **an enumeration produced by a first-abort
instrument is a frontier, not an inventory** — its completeness claim is conditional
on every earlier frontier staying closed, and the honest phrasing is "the next
verdict", never "the remaining list". The class lesson is F71's, now three times over:
the fork's remaining hazards are not exotic aliasing but ordinary library-method
autorefs (`as_mut_ptr` on a shared field) and per-slice re-stamps of frame-constant
values; both are findable by grep (`\.as_mut_ptr()` on ctx-reached state inside the
MB loop) faster than by 28-minute probe iterations, which is how rounds 8's four
sites were closed in one pass.

## F144 — the detector's session-E ledger: one false-positive class confirmed, one
new artifact class, and the dispatch blind spot observed live

Three instrument observations from driving `q1c.py --type SSlice` from 68 to 0 and
scoping `--type SDqLayer` (F123's discipline: understand the instrument before
arguing with it):

1. **A held *bool* is not a held cursor** — 11 of the layer family's 14 reported
   hazards are `let bLeft = … && uiSliceIdc == WelsMbToSliceIdc(pLayer, …)` followed
   by another `WelsMbToSliceIdc` call and a later read of `bLeft`. The "cursor" is a
   `bool`; a value cannot be invalidated by a retag. The layer family's real
   pre-flip work is its 3 shape-B argument-order sites, not 14 sites.
2. **A re-derivation spelled as an assignment reads as a use** — `pSliceBs =
   addr_of_mut!(…)` after a crossing call is reported as "`pSliceBs` read after the
   call": the write to the local counts as a mention. The `let`-shadow spelling of
   the same fix reads clean, and is what the tree now uses.
3. **The dispatch blind spot bites in exactly the way F111 predicted** — the inter
   loops call `pfInterMd` through the slot with `pSlice` as an argument;
   `q1c` attributes nothing to it, but its function type carries `*mut SSlice`, so
   it is a crossing like any named callee, found by reading. The step-3 windows in
   both inter loops are placed after it, with a comment naming the blind spot.

Neither matcher was changed: S55's second edge requires any detector change to
re-calibrate against `0bfc7687^`, and this session had rounds in flight; the
narrowing decision passes to the steward with this ledger.

## F145 — the E2 brief's counts: nine slice-carrying typedefs, not six; "107 parameter
mentions" is a contaminated grep; and the true parameter inventory was 93

Three of the brief's load-bearing numbers, re-measured at its own commit before any
edit (its opening instruction):

1. **"The worker's count is six typedefs carrying `*mut SSlice`"** — the multiline-
   aware scan the same sentence prescribes finds **nine**: beside the two it names
   (`PDeblockingFilterSlice`, `PWelsCodingSliceFunc`) and the two it hints at
   (`PMotionSearchFunc`, `PSearchMethodFunc`), also `PWelsSliceHeaderWriteFunc` and
   the four MD slot types in `wels_func_ptr_def.rs` — `PInterFineMdFunc`,
   `PInterMdBackgroundDecisionFunc`, `PInterMdScrollingPSkipDecisionFunc`,
   `PInterMdFunc` — every one wrap-hidden past a one-line grep, which is F101's
   lesson biting the very sentence that cited it. All nine flipped in stage 1's S20
   closure; the installee sets pulled `WelsDiamondSearch`, `WelsMotionCrossSearch`
   and `WelsDiamondCrossSearch` up a level with `PSearchMethodFunc`.
2. **"82 bodies, 107 parameter mentions"** — 82 bodies is exact; 107 is the line
   count of `grep ': \*mut SSlice'`, which without a word boundary also matches
   `*mut SSliceThreading` (`encoder_context.rs:1091`), and with comments and locals
   included counts three worker locals and eleven prose mentions. The parameter
   inventory the campaign actually converts was **93**: 84 fn-signature positions
   plus 9 typedef positions (and of the 84, two left by S54 deletion rather than
   flip — `WelsMarkMMCORefInfoWithBase`'s base became a value, `mb_dump`'s
   `*const` became `&SSlice`).
3. Smaller: `sRefPicView` "21 reads" measures 22 code mentions at the close (the
   harvest was dropped at the frontier; the successor re-measures).

## F146 — the layer flip's real shape is a fork-reachability split: the brief's step
2 contradicts its own edge-2 rule, and 43 of the 72 layer-param definitions must
stay raw until G–H

The brief's step 2 says "`*mut SDqLayer` → `&mut SDqLayer` the same staged way — 73
mentions"; its own edge 2 says "no `&mut` to the slice **array** (or the layer that
owns it) inside the fork, ever." Both cannot hold: the layer is the one object every
worker shares, and a `&mut SDqLayer` parameter on any fork-reachable function makes N
workers perform concurrent entry retags of one object — F73 across threads, the class
rounds 5–9 spent the previous session evicting. The in-fork surface is not a corner:
a call-graph closure seeded at the three `thread::scope` workers and every dispatch-
slot installee the fork calls through reaches **42 of the 71 layer-param bodies** —
the MD partition searches, the ME entries, the header writers and their init pair,
deblocking's slice filter, the slice services (`SetSliceBoundaryInfo`,
`ReallocateSliceInThread`/`List`, `UpdateMbNeighbor` and both walkers,
`WelsGetNextMbOfSlice`, `WelsMbToSliceIdc`) and every accessor. Those, plus
`slice_in_layer` (kept with its accessor family), stay raw: **43 definitions, G–H's
layer inheritance**. The other **30 definitions** run only outside the fork — the
init, teardown, between-frames-adjust, and ref-marking families plus the frame
deblocking walk — and flipped this session (28 `&mut SDqLayer`, 2 `&SDqLayer`).

The classification's referee is the close's Miri (the probes drive both fork paths
and the ST paths); its one catch is F148's third item. What G–H inherits beyond the
list: the in-fork layer writes are per-worker-disjoint fields of one shared struct
(`sSliceBufferInfo[slot]`, the partition arrays), which is interior-mutability
territory (rec_view's shape) or permanent raw — a design decision, not a flip.

## F147 — the popped-sibling-argument class: a cursor passed INTO the same call
beside its own root, invisible to all four detector shapes; S54 narrowing is the fix

A caller that passes the slice AND a slice-derived cursor in one call is sound while
both are raw and UB the moment the slice parameter becomes `&mut`: the argument
reborrow is a whole-object write retag that pops the sibling cursor **before the
callee runs**, wherever the cursor's bytes live inside the object. q1c cannot see it
in either kind: shape B's "through-read" pattern requires a deref of the root
itself (the cursor is one derivation removed), and shape A requires a use *strictly
after* the call — the use here is *at* the call. Five instances, all closed by
narrowing (S54) rather than windows, because in each the cursor was a pure function
of the other arguments:

* the header writers and `PWelsSliceHeaderWriteFunc` lost `pBs: *mut BsWriter`
  (derived inside via `slice_writer` under the parameter's own tag);
* `WritePrefixNalForSlice` lost `pSliceBs: *mut SWelsSliceBs` the same way;
* `WelsCodeOneSlice`'s `pBs` moved below the two slot calls whose reborrows popped
  it, minted at its one remaining use;
* both P-macroblock loops (and the cavlc syntax writer, forward-proofed at its own
  stage) re-mint their writer after the loop's whole-slice passes;
* `WelsMarkMMCORefInfoWithBase` takes the base *marking by value* — the variant
  where the protector itself is the problem: iteration 0 writes the very bytes a
  reference parameter would protect (its own S29 comment documented the self-copy),
  and a value cannot be invalidated by a retag.

The general rule for every future flip: at each call-site rewrite, read the OTHER
arguments of the same call for derived cursors into the object being reborrowed.

## F148 — a flip's blast radius is caller-side and read-side: three instrument gaps
this campaign hit, one of which only the close's Miri saw

1. **A typedef flip creates held-cursor UB in still-raw callers** (S52's atomicity
   has a blast radius the stage plan must own). When stage 1 flipped the ME slot
   types, the still-raw loop bodies `WelsMdP8x8`/`P16x8`/`P8x16` were suddenly
   holding `pMbCache` across a `&mut *pSlice` argument — and using it after, in a
   loop, so even the next iteration's pre-call reads were through a popped tag.
   F114's C/D class ("hazards the conversion creates") extends to the *callers* of
   a flipped dispatch surface; q1c is blind in both kinds (dispatch slot, F111;
   raw-param root, F144's third limit). Fix: per-iteration derivation windows,
   placed in the same commit as the typedef flip.
2. **Shape B is blind at accessor-minted roots** (the tool's documented first
   limit, now with teeth): four hand-found same-call hazards — `NeedDynamicAdjust`'s
   `iSliceNumInFrame` argument at three sites, one of which can pass THE SAME layer
   as both arguments (base == current), and
   `DynslcUpdateMbNeighbourInfoListForAllSlices`' `mb_list_root` argument, fixed by
   reorder (the MB buffer is a separate allocation, so the retag cannot reach it).
   The layer campaign's roots are almost all `current_layer(pCtx)` mints, so for
   G–H this blindness is the norm, not the exception.
3. **A protector is violated by a foreign READ, and shape D only models mints.**
   The close's session gate — the session's single Miri run, per the user's
   directive — aborted both encode probes at one line:
   `WelsUpdateSliceHeaderSyntax`, freshly `&mut SDqLayer`, read
   `(*current_layer(pCtx)).iMaxSliceNum` — the same object its own parameter
   protects, reached by the ctx path. No `&mut` is minted anywhere: a plain read
   through an independent same-tag copy pops a strongly protected Unique. Shape D's
   closure looks for reborrow mints and cannot see reads, so the site survived a
   clean `--kind ref` scan. Fixed by T9.D11's move (read through the parameter; the
   caller hoists the old null arm), confirmed by re-running exactly the two failed
   probes. Instrument note for the steward: shape D's "mint" model needs a
   read-through-independent-path clause for ref-param bodies whose type has
   ctx-resident accessors — with S55's calibration cost, this session left the
   matcher alone and hands the ledger forward.

## F149 — `md_cost_raw`/`me_cost_raw` had zero callers from birth; the brief's "16 uses outside md.rs" matches nothing measurable

The F brief's step-2 list included "the `md_cost_raw`/`me_cost_raw` accessors
(`md.rs:837`/`:848`, **16** uses outside `md.rs`)" as conversion work. Measured at
the brief's own commit:

```
$ grep -rn 'md_cost_raw\|me_cost_raw' src tests --include='*.rs' | grep -v 'src/encoder/md.rs'
(nothing)
$ grep -rn '\.md_cost(\|\.me_cost(' src | grep -v md.rs | wc -l
3
```

The raw pair had **zero callers anywhere in the tree** — every runtime-selected
cost site went through the safe `md_cost`/`me_cost` from the moment B2–B3 flipped
it, so T9.B25's transitional accessors were dead the day they were written. And the
safe pair has **3** uses outside `md.rs`, not 16; the brief's number corresponds to
no grep this tree can produce. Consequence for the session: an S18 deletion where
the brief had scheduled a conversion — strictly less work, but the class matters:
**a brief's workload number for an accessor is a grep, and quoting it without the
command beside it is how a phantom 16 survives review** (S24's rule, restated for
briefs). The deletion grep is quoted at the site in `md.rs`.

## F150 — the `pfMotionSearch` loop installs one function into every slot; the screen-content variants have no installer at all

The brief: "The installs (`encoder_ext.rs:2446`, a loop over screen-content
variants) update signatures with the typedef." The tree: the loop installs
`WelsMotionEstimateSearch` — the one live search — into **every**
`BLOCK_STATIC_IDC` slot, unconditionally, on every P-frame preprocess:

```rust
for i in 0..EStaticBlockIdc::BLOCK_STATIC_IDC_ALL as usize {
    fl.pfMotionSearch[i] = Some(crate::encoder::svc_motion_estimate::WelsMotionEstimateSearch);
}
```

`WelsMotionEstimateSearchStatic` and `..Scrolled` have **zero install sites** — the
C++ block that installs them per static idc (`encoder_ext.cpp:2708-2771`, the
SCREEN_CONTENT arm of `PreprocessSliceCoding`) is explicitly untranslated, a fact
the port records at the site and F125's table mis-attributed to `WelsInitMeFunc`.
Three consequences, all landed this session: (a) the runtime-indexed dispatch
`pfMotionSearch[iBlock8x8StaticIdc[i]]` is **double-locked** — every entry holds
the same function, and the index's only nonzero writer (`SetBlockStaticIdcToMd`)
is SCREEN_CONTENT(dormant) — so the de-virtualized call is byte-identical without
any constancy argument on the index; (b) `pfSearchMethod`'s only writer is the
sibling loop installing `WelsDiamondSearch` for every block size, because (c)
`SetMeMethod` — the ME_DIA/CROSS/FME selector the C++ reaches only from that same
untranslated block — had **zero callers** and is deleted (S18). The dormant search
variants stay, converted to the new typedefs, for Phase 10 to re-install.

## F151 — the brief's "sad_common denies with zero allows" collides with its own "preprocess is not this session"; resolved by C2's ownership rule

The 14 raw SAD shims' read grep (S18, tests and benches included — F119's clause,
paid once more by this session's step 0 before it was re-learned) found **15**
callers, not 14 + tests: `processing/scene_change_detection.rs:87` — the scene
change detector's 8x8 block walk, the plane census's `preprocess` row — calls
`WelsSampleSad8x8_c` on raw `SPixMap` pixel pointers that no cursor can replace
until the preprocess family's own session converts the pixmap. The brief demands
both "`sad_common.rs` denies with zero allows" and "not this session: the
preprocess family", and lists neither the caller nor the tension.

Resolution: **the raw body moves to the family that owns its last caller** —
F139's ownership rule for the decoder's deblocking shims, applied to the
preprocess family. `sad_8x8_raw` is a private, flattened (exactness argued from
associativity at the site), `port-raw`-tagged kernel inside
`scene_change_detection.rs`; `sad_common.rs` reaches `#![deny(unsafe_code)]` with
zero allows. The ratchet was **rebaselined** for that one file (+2 `raw_ptr`,
+3 `unsafe_block`, +1 `unsafe_fn`) in the same commit, reason stated — the only
per-file increase in a session that took the crate down 118 `raw_ptr`. The
preprocess session inherits one raw kernel where it used to inherit a
cross-module dependency.

## F152 — two census blind spots: an interior-pointer slot read is invisible, and a relocated kernel takes its row with it

Two ways `phase9_plane_callers.py`'s picture diverged from the tree this session,
both one class: **the census keys on spellings, and both fixes changed the
spelling without changing the fact.**

1. `CheckChromaCost`'s cost-table read was respelled by T9.E7 (F132 round 8) as
   `addr_of!((*fl).sSampleDealingFuncs.pfSampleSadRaw).cast::<..>()` — an interior
   pointer handed to `GetChromaCost` — and from that commit on the census had **no
   row for the encoder's one remaining live raw cost read**: it lists direct slot
   mentions at call sites, and the slot's mention had moved into a cast expression
   two frames up the call chain. The brief scoped step 2 off the census and
   therefore never listed `CheckChromaCost`/`GetChromaCost`, the largest live
   conversion in the step (F137's lesson, third instance: the census's kernel and
   slot lists are enumerations, and every respelling is a chance to fall out of
   them).
2. The `preprocess` row (`Process`, `WelsSampleSad8x8_c`) disappeared from the
   census at T9.F2b not because the site converted but because the kernel it calls
   is now a private local name the census does not enumerate. The census's
   `safe-now` column closing at 6 rows is right about the encoder and silent about
   the relocation; the log states it so the row is not read as converted.

Session F's close census: 27 sites — 6 `safe-now` (3 `VaaBackgroundMbDataUpdate`
copies under F117/S57's stay-raw ruling, 3 dormant `SvcMdSCDMbEnc` MCs — all
Phase 10's or F117's), 13 `coeff`, 3 `blocked`, 5 `seam`, 0 unclassified. The ME
rows are gone: the brief's step-2 prediction ("only the preprocess row and Phase
10's") is right in substance with the two corrections above.

## F153 — F84's "two dead threaded-decoder functions + 18 pad kernels" is stale in two of its three claims, and the stale copy propagated through two briefs

F84 (Phase 8b session A) recorded `WelsDecodeAndConstructSlice` and
`WelsDeblockingFilterMB` as S18 candidates with zero callers, plus the 18
`MBPad*_c`/`PadMB*_c` kernels "only they reach". The D-scope-1 board and the E3
charter row inherited all three claims. Measured at E3's step 0
(`96b68a91`), only one survives:

1. **`WelsDeblockingFilterMB` was deletable and is deleted** —
   `grep -rn 'WelsDeblockingFilterMB' src tests benches | grep -v 'fn WelsDeblockingFilterMB'`
   → 0 before the deletion. Its one C++ caller is `decode_slice.cpp:1727`, inside
   the untranslated deblock-as-you-go arm.
2. **`WelsDecodeAndConstructSlice` is not deletable and is not deleted.** T7.C7
   (after F84 was filed) *kept and fenced it*: `decoder_core.rs:4696` calls it
   under `iThreadCount > 1` behind the documented `DECODER_MT(incomplete: F36)`
   fence, through the wrapper at `decoder_core.rs:824`, and two tests drive the
   wrapper (`decoder_core.rs:5254`, `decode_slice.rs:5596`). Deleting it would
   reverse a recorded decision. F84's zero-caller claim was true when filed and
   falsified by a later session's work — the class S24 names: a brief inheriting
   a finding inherits the finding's *date*.
3. **The 18 pad kernels were never ported**: `grep -rn 'MBPad\|PadMB' src` → 0.
   There was nothing to delete; "the 18 kernels only they reach" described the
   C++, not the port.

`WelsGetFirstMbOfSlice` (deleted in the same commit) is the cleaner story the
charter row folded in beside F84: dead in **both** trees — the C++ defines it at
`svc_enc_slice_segment.cpp:540` and never calls it.

## F154 — the grid's 34 was three counts short, and the missing three are the ones a `*mut SMB` grep cannot see

The E3 brief scoped the grid at "**34** `*mut SMB` parameters" with its command
quoted, and that command is right about what it asks. The family is bigger,
because three sites spell the same relationship differently:

```
$ grep -rn ': \*const SMB\b' src/encoder | grep -v ':\s*//'
svc_mode_decision.rs:1487   SetMvBaseEnhancelayer's kpRefMb
svc_encode_slice.rs:2046    WelsInitInterMDStruc's pCurMb
svc_base_layer_md.rs:2137   WelsMdInterSaveSadAndRefMbType's pCurMb
$ grep -rn '\-> \*mut SMB' src/encoder            # GetRefMb, mb_at, mb_list_root
$ grep -rn 'pub type PMb' src/encoder             # deblocking.rs:237
```

Three `*const` parameters, one raw **return** (`GetRefMb`, the cross-layer
read), and a type alias with zero users. None is exotic: the `*const` three are
neighbour-family functions whose bodies only read, `GetRefMb` is how the
enhancement layer reaches its base layer's record, and `PMb` is a leftover
spelling of the family's own type. All five converted or died with the 34, and
the session's exit condition became `grep -rn '\*mut SMB\|\*const SMB' src tests
benches` → **0** rather than the brief's number reaching zero.

The general clause, and it is S24's unit rule pointed at *mutability*: a family
scoped by one pointer spelling is scoped by a **grep, not by the relationship**.
The question "how many places hold a raw macroblock record" has four spellings
in this tree (`*mut T`, `*const T`, `-> *mut T`, `type Alias = *mut T`), and a
brief that quotes one of them is quoting a lower bound. Enumerate the *type*,
not the parameter form — the same way F101 says to enumerate the whole item and
not the first line.

## F155 — a QP-average neighbour read is byte-inert at one-macroblock granularity: the first planted fault proved nothing, and the reason generalises

Deblocking's grid conversion was calibrated per S55 by skewing a neighbour
lookup one macroblock right. The first attempt skewed the **QP average** —
`(iCurLumaQp + left/topMb.uiLumaQp + 1) >> 1`, `DeblockingInterMb`'s filter
strength input — and the honest count was **st 210/210 PASS, dl 76/76 PASS**:
zero rows, on the two presets that exercise deblocking hardest.

The cause is not coverage. QP is assigned per *GOM row* (`bGomRC`,
`iGomSize`), so at every interior macroblock the "wrong" neighbour holds the
**same QP**, the average is unchanged, and the filter runs identically. A
one-macroblock skew is invisible to any read of a value that varies more slowly
than one macroblock.

Re-planted in the same function family at the **BS-calc top record** — whose
inputs are `uiMbType`, `iNonZeroCount` and `sMv`, all genuinely per-macroblock —
the same skew fails **st 180 of 210** and **dl 76 of 76**. Both counts are
quoted in T9.E3c's message.

S59 says an inert row referees nothing at that site whatever the entry counter
says; this is the same law one level down, at the *field*: **a planted fault
proves coverage of the field it perturbs, not of the site.** Choose the
perturbed field for its variation, not for its convenience — and when a fault
comes back 0/210, the first hypothesis is an inert field, not an unreached path.

## F156 — `sDecPicView` had zero readers, and the field that survived it needed no struct at all

The E3 brief scoped the harvest as "the two raw picture views re-routed onto
per-call cursors", with `sRefPicView` at 23 mentions and `sDecPicView` at 4.
Measured at conversion time, the twin is not a re-routing job:

```
$ grep -rn 'sDecPicView' src | grep -v '^\s*//'
svc_encode_slice.rs:1132   the declaration
svc_encode_slice.rs:1228   its Default
encoder_ext.rs:1969        the null-stamp arm
encoder_ext.rs:1976        the stamp
```

Four mentions, **all writes**. The reconstruction picture's readers moved to the
seam in T9.C2/C4 (`layer_rec_view`, `pCsData`/`iCsStride`), and the view field
kept being stamped for nobody — a write-only struct field, F139's write-only
*slot* class one container over, and undetectable by the same means (it is
`pub`, in a `pub` struct, so the compiler's dead-code pass says nothing —
F129's rule again).

`sRefPicView`'s 23 are real, and they resolve into fewer moving parts than the
count suggests: 13 are one stamp block's plane/stride reads, 4 are the dormant
screen-content pointer, 2 are `iPictureType` guards, and 4 are comments. What
the struct actually bought was **one exclusive-borrow avoidance** (T6.F5:
`SPicture::view()` needs `&mut`, and per-call resolution in-fork would be F73's
retag per worker) — and that is a property of the *accessor*, not of the data.
Two `&self` root mints (`PaddedPlane::root_ptr_shared`,
`SPicture::data_ptr_shared`, F71's argument transplanted from `MbArray`) remove
the reason, after which both fields, the struct, its constructor and its stamp
all delete together.

The clause worth carrying: **before re-routing a cached copy, grep its readers
per field.** A "pair of fields" in a brief may be one field with readers and one
with none, and the second costs nothing but is invisible until asked.

## F157 — the fork-reachability walker's answer is decided by an arm no signature census has: function-item arrays

S63 tells G's brief to carry the ctx family's fork-split, and E2 produced the
layer's by hand. Built as a tool (`phase9_forksplit.py`), the first honest run
reported **27 of 268 bodies in-fork**, with `svc_base_layer_md.rs` and
`svc_mode_decision.rs` at **zero** — which is absurd on its face: mode decision
is what the fork's workers spend their time in.

The walker knew two dispatch arms — named calls and `pf*` slot installees — and
the encoder reaches mode decision through neither. `WelsCodeOneSlice` indexes a
**static array of function items**:

```rust
pub static g_pWelsSliceCoding: [[PWelsCodingSliceFunc; 2]; 2] = [ … ];
let func = g_pWelsSliceCoding[idr_idx][kiDynamicSliceFlag];
let iEncReturn = func(pEncCtx, &mut *pCurSlice);
```

With that arm added the split is **111 in-fork / 157 ST-flippable**, and the
`--why` paths check out by hand. The arms are worth 84 bodies, which is the
calibration the tool ships with (`--no-slots`: 27 vs 111).

Two things generalise. First, **a table is a dispatch surface even when it is
not a struct field** — S52 said "read the callers of the table, not of the
kernel" about `SWelsFuncPtrList`; a `static` array of `fn` items is the same
object with no owner to key on, and a census that greps for `.pf` cannot see
it. Second, and this is the reason to write the walker rather than repeat E2's
hand-count: **the hand-count and the tool disagreeing is the useful event.** The
hand method has no way to report which arm it forgot.

## F158 — where the encoder's per-macroblock reads now live, for G and X

A closing inventory, since after E3 the encoder's per-macroblock world has one
raw spine left and the next sessions should not have to re-derive its shape.

**Gone**: every raw macroblock-record access. `grep -rn '\*mut SMB\|\*const SMB'
src tests benches` → 0; records are reached through `MbWindow` (a borrowed range
of the grid, with the window's bounds as the fork-disjointness mechanism) or
through `&SMB`/`&mut SMB` where a single record is the whole subject.

**Gone**: the layer's two stamped picture views; the reference picture resolves
per call.

**What still carries raw, per file, and whose it is**:

* `*mut sWelsEncCtx` — 285 mentions / 268 bodies, split 111 in-fork / 157 ST
  (`phase9_ctx_forksplit.md`). **G's**, and the split is the plan, not the
  hazard count.
* `*mut SDqLayer` — the in-fork half of E2's split, unchanged this session
  except that E3's mints take it as a parameter. **G's**, and it moves with the
  ctx: both are "the object every worker shares".
* `*mut u8` VAA flag walks (`pVaaBgMbFlag` in the two inter fills), `*mut i16`
  residual cursors (`WelsWriteBlockResidualCabac`'s `pBlock`), `*mut u16` MVD
  cost tables. **X's**, all three, and none is grid-shaped: they are flat
  per-macroblock arrays indexed by `iMbXY`, which is the `SharedMbArray` shape
  the seam already uses for `pMbSkipSad`.
* The dormant screen-content pointer, now behind one named accessor
  (`layer_ref_feature_storage`). **Phase 10's.**

## F159 — the stale-driver guard was aimed at the cargo crate *directory*, so it reported STALE on every run and had failed both `gates.sh` sweeps since it landed

`583cd21a` added a stale-driver guard to `sweep.sh` after E3 lost two hand-run
fault probes to an old binary. Sound idea, wrong path:

```
$HERE/rust_enc                          <- the cargo crate DIRECTORY
$HERE/rust_enc/target/$PROFILE/rust_enc <- the binary (compare.sh:35 spells it)
```

Both arms of the guard died on that:

```
$ [ -x rust_enc ] && echo TRUE                 # directories are traversable
TRUE                                           #  -> "binaries missing" never fires
$ find …/src -name '*.rs' -newer rust_enc | wc -l
83                                             # vs the DIRECTORY's mtime — Aug 22,
                                               #  when a file was last added to the
                                               #  crate — so always STALE
$ find …/src -name '*.rs' -newer rust_enc/target/debug/rust_enc | wc -l
0                                              # the tree was fresh the whole time
```

So every `sweep.sh` invocation after that commit exited 2 without probing a byte,
including the two inside `gates.sh family/session/full/exit`. **The byte-identity
gate — the phase's central rule, and the thing every session's "583/583" quotes —
was down from `583cd21a` until this session fixed it.** Session G's brief carried
the claim forward as "gates.sh is unaffected (build.sh runs first at
gates.sh:356)": build.sh does run, and does rebuild, and the guard was not
comparing against what it built.

**The generalisable part is S55's other half, and it is not "calibrate the
detector" — it is *which* calibration.** S55 as written says to plant a known
positive and watch the instrument fire. This guard was calibrated exactly that
way: its commit records "First live run fired truthfully: rust_enc is stale at
HEAD right now." That sentence is the false positive being read as a
confirmation. A detector that fires on everything passes a positive-only
calibration perfectly. **The arm that finds this class is the negative one: show
the instrument SILENT on a case you know is clean.** It costs one run and it is
the only arm that distinguishes "detects the fault" from "detects nothing".

Fixed and calibrated in both directions, six arms — fresh/debug and fresh/release
silent with `PASS=11 FAIL=0`, touched source fires and names the file, debug-fresh
/ release-stale fires for release only (a new arm: the old guard had no profile,
so one fresh driver vouched for both), moved binary fires with the path,
`SWEEP_STALE_OK=1` still overrides. Exit code checked directly rather than through
a pipe: 2 on stale, 0 on fresh.

**Where else this reaches.** Every instrument this phase ships is a candidate:
`q1c.py` and `phase9_forksplit.py` both have a documented positive calibration
(F157's `--no-slots` 27-vs-111) and neither has a recorded negative one. A
forksplit that reported every body in-fork, or a q1c that reported every call a
hazard, would read as "conservative" and would pass every check either tool
currently ships.

## F160 — D-dead-3 leaves two orphans, both of the shape the ruling was about, and neither is this session's to delete

Deleting `pGomCost` took its justifications with it, and two things in the tree
were standing on them.

**1. `SharedMbArray::capture` (`encoder/rec_view.rs:342`) has no caller.** Its doc
said it was public "because the reconstruction picture is not the only owner of
one: the rate controller's `pGomCost` has the same shape and the same fork". The
rate controller never took it — T9.C5 made the field `Vec<AtomicI32>` instead —
and every construction in `rec_view.rs` builds the struct literal directly
(`SharedMbArray { cells: SharedCells::capture(…) }`, 7 sites). So the method was
dead at the moment its justification was written, and `grep -rn '::capture('`
finds only `SharedCells::capture`.

**2. The three `pfIDct*` slots are installed, asserted-installed, and called by
nothing** (F138, recorded at `decode_mb_aux.rs:463`). That note left them alone
and said why: "the F133 ruling was to leave write-only storage alone." **D-dead-3
reversed that ruling.** Its ground — a write with no reader is not state — reaches
a dispatch slot with no caller verbatim, so F138's precedent is now the opposite
of what its comment says.

Both are recorded, neither is touched. The ruling that reached `pGomCost` is the
user's, and it was made on `pGomCost`; extending it to two more members of the
same class is a second ruling, not an inference a session gets to make. Both
comments now point here instead of at a precedent that has flipped.

## F161 — the hazard detector's output has to be split by fork-reachability too: 135 of 266 reported hazards model a retag S63 forbids, including all 55 against the brief's centrepiece

Session G's brief scopes step 2 as "hazards to zero": `q1c.py --kind raw` reads
266 hazardous sites in 69 callers, and the campaign drives that to 0 before any
signature moves. It names the remedy as narrowing the wide accessors, and names
the largest one: *"Narrowing `ctx_param` alone implicates 55 sites."*

`ctx_param` is in the **in-fork** column.

```
$ python3 rust/tools/phase9_forksplit.py --why ctx_param
ctx_param <- WelsCodeOneSlice <- EncodeOnePartitionSizeLimited <- fork seed
```

S63 (F146) is that an in-fork body **cannot take `&mut` by any amount of hazard
work**; its end states are interior mutability or lawful raw. q1c's own docstring
is equally explicit about what it reports: *"It models one conversion — the
parameter becomes `&mut T`."* Put together: a hazard whose **callee** is in-fork
models a retag that will never be taken. It is not UB today either — F66's whole
finding is that a raw entry retag pops only the offsets it touches, which is why
the port's dominant idiom is sound and why the tree is green under Miri with all
266 present.

Joined (`rust/tools/phase9_ctx_join.py`, shipped with this finding):

| | sites | shape A | shape B | held cursors |
|---|---:|---:|---:|---:|
| **live** — callee flips, the retag really happens | **131** | 95 | 36 | **17** |
| **moot** — callee is in-fork, S63 forbids the retag | **135** | 110 | 25 | 41 |
| total (what q1c reports) | 266 | 205 | 61 | 44 |

Half the prescribed campaign is not work, and the half the brief singled out is
the moot half: `ctx_param` 55, `ctx_ref_list` 19, `current_layer` 19, `ctx_rc_at`
10, `ctx_vaa` 9, `ctx_func_list` 7 — every one in-fork, 119 sites, and the brief's
named remedy (narrowing `ctx_param`) buys **zero** live sites.

The live campaign is also a different *shape*, which is the part that matters for
scoping. Its 95 shape-A sites stand on **17** (file, caller, binding) cursors and
only **four distinct names** — `pSpatialIndexMap`, `pLtr`, `pParamInternal`,
`pParamD` — with 58 of the 95 being one cursor in one function
(`pSpatialIndexMap` in `WelsEncoderEncodeExt`). "44 cursors retired by accessor
narrowing" and "17 cursors retired by re-derivation, 4 of them by name" are not
the same session.

**The class.** This is F146 in the mirror. E2's brief ordered a `&mut` flip *on*
bodies the fork shares; G's brief ordered hazard-clearing *for* bodies the fork
shares. Both come from applying the split to the family and then reading the
detector as if the split did not exist. S63 says the split is the first number a
brief must carry — the missing clause is that it is also the first number the
**detector's output** must be filtered by, because two instruments that do not
know about each other cannot do it for you. A hazard report is a conditional:
*if* this converts, this breaks. Counting conditionals whose antecedent is
forbidden is counting.

## F162 — an upstream one-past-the-end read the port reproduces, at `WelsEncoderEncodeExt`'s error tail

`encoder_ext.rs`, at the `ENC_RETURN_CORRECTED` branch after the spatial loop:

```rust
if ENC_RETURN_CORRECTED == (*pCtx).iEncoderError {
    let iDid = (*pSpatialIndexMap.add(iSpatialIdx as usize)).iDid;
```

`iSpatialIdx == iSpatialNum` there — the loop ran to completion — and
`sSpatialIndexMap` is `[SSpatialPicIndex; MAX_DEPENDENCY_LAYER]` with
`MAX_DEPENDENCY_LAYER = 4`. With four spatial layers configured (`dl` sweeps 2/3/4)
the read is one past the end.

Upstream does the identical thing, twice, at `encoder_ext.cpp:4109-4110`:
`(pSpatialIndexMap + iSpatialIdx)->iDid` as the fourth argument to
`UpdateSpatialPictures` and the second to `ForceCodingIDR`.

**Left as upstream has it.** T9.G2 replaced every other use of that cursor with a
direct index; this one keeps the raw spelling because an index panics where a
pointer reads, and a panic is not byte-identical. It is spelled as a derivation
that lives and dies inside the statement, so it costs nothing in hazard terms. The
site is commented and points here.

Whether the port should diverge from upstream on a latent out-of-bounds read in an
error path is a ruling, not a session's call — the same shape as D-dead-3, and
recorded the same way.

## F163 — "drive the hazard detector to zero" is not a reachable exit criterion: its own remedy for shape B produces shape A

Session G's brief scopes step 2 as `q1c --kind raw` → **0 across all shapes**. The
campaign got 266 → 92 and live 131 → 7, and the last stretch showed why 0 is the
wrong target.

q1c reports two shapes. **A** is a cursor held across a call; its remedy is to
retire or re-derive the cursor. **B** is an argument evaluated after the callee has
taken the retag; its remedy is to hoist the argument into a binding before the
call. The two remedies are in tension:

- hoisting a **scalar** clears the site — `iRCMode`, `iMaxSliceNum`,
  `iActiveThreadsNum`, `uiDependencyId`;
- hoisting a **pointer** converts B into A — `let l = &mut *current_layer(pCtx);
  f(pCtx, …, l, …)` is read as a cursor derived, then a call, then a use.

Three of T9.G6's eighteen hoists moved that way, and all three are the *correct*
end state: `current_layer` is fork-reachable, so S63 keeps it `*mut` permanently,
the `&mut` lands on the layer allocation rather than on a borrow of the context,
and the hoisted form is what compiles once the callee flips. The unhoisted form is
what does not. For a context whose accessors return pointers, that case is most of
them.

**The deeper reason 0 is unreachable is that q1c models the retag by type and the
hazard is by allocation.** A `&mut sWelsEncCtx` entry retag covers
`[0x0..0x17ee8]` — the context's own allocation. F66's Miri trace names exactly
that, and the cursor it killed was `sSpatialIndexMap`, an **inline array field**
inside the context. A cursor that points into a `Vec` buffer or a `Box` the context
merely holds a pointer to is in a *different allocation* and cannot be reached by
that retag. The port's accessors are written to launder provenance out of the
container on purpose (F71, "shared access to the `Vec`, buffer provenance out"), so
most context cursors are of the second kind. All 7 live sites left at G's close are:
4 × `pParamD` into the coding-param `Box`, 3 × T9.G6's hoists.

The reachable criterion is **the join's live count, read per site** — and the
useful instrument would be a q1c that classifies a derivation by the allocation it
lands in (inline field vs. container payload), which is a static question its
existing `DERIV_HINT` machinery is already most of the way to answering. Until then
"0" should not be written into a brief as an exit condition.

This is F161's sibling: F161 is the detector over-reporting because it does not
know the fork split; F163 is the detector over-reporting because it does not know
the allocation. Both are the same lesson — **a hazard report is a conditional, and
the brief must filter it before quoting it as a work list.**

## F164 — F67 re-derived at HEAD: still twelve, but a different twelve, and only four of them are G's

The send-seam's own comment (`slice_multi_threading.rs:1245`) names its retirement
condition and its evidence: `sWelsEncCtx` is `!Sync` for **twelve** distinct
reasons, "five of them inside types Phases 8 and 10 own". That inventory is Phase
7's. Re-derived at HEAD with the same probe F67 used:

```rust
fn _probe_assert_sync<T: Sync>() {}
fn _probe_ctx_sync() { _probe_assert_sync::<sWelsEncCtx>(); }
```

**12 `E0277`s again — and the membership has moved.**

| blocking type | reached through | whose |
|---|---|---|
| `*mut SSliceThreading` | `sWelsEncCtx` directly | **G/H** |
| `*mut SWelsEncoderOutput` | `sWelsEncCtx` directly | **G/H** |
| `*mut SRefList` | `SDqLayer` → `Box` → `Option` → `Vec` → ctx | **G/H** |
| `*mut SrcPicPool` | `SDqLayer` → … → ctx | **G/H — new, not in F67** |
| `*mut CWelsPreProcess` | `sWelsEncCtx` directly | preprocessing |
| `*mut c_void` | `SLogContext` → ctx | trace |
| `*mut i8` | `SWelsSvcCodingParam` → `Box` → `Option` → ctx | **Phase 8's** |
| `*mut u8` | `SVAAFrameInfo` → `Box` → `Option` → ctx | **X's** |
| `*mut SMotionTextureUnit` | `SAdaptiveQuantizationParam` → `SVAAFrameInfo` → … | **X's** |
| `*mut i32` | `SComplexityAnalysisParam` → `SVAAFrameInfo` → … | **X's** |
| `*mut u32` | `SComplexityAnalysisParam` → `SVAAFrameInfo` → … | **X's** |
| `*mut SScreenBlockFeatureStorage` | ctx | **Phase 10's** |

Three changes since Phase 7:

1. **`*mut CMemoryAlign` is gone**, exactly as F67 predicted ("retires with the last
   two allocator sites in session B").
2. **`*mut SrcPicPool` is new**, inside `SDqLayer` — it arrived with the layer's
   picture-pool work after F67 was written. The count held at twelve by
   coincidence, which is worth saying out loud: a stable total is not a stable list.
3. **The `[*mut u8; 4]` field F67 named on the context itself is gone**; `*mut u8`,
   `*mut i32` and `*mut u32` now all reach through `SVAAFrameInfo`. F67's table
   attributed them to the context directly.

**The verdict for the send-seam: G cannot retire it, and no amount of G's work
can.** Only 4 of the 12 are G's; the other 8 are `SVAAFrameInfo`'s four (X's),
`SWelsSvcCodingParam`'s one (Phase 8's), `SScreenBlockFeatureStorage` (Phase 10's),
`SLogContext`'s `*mut c_void`, and `CWelsPreProcess`. Converting every ST body and
building the whole in-fork read surface leaves `sWelsEncCtx` `!Sync` for eight
reasons that are not in this session's lane. The brief's own branch applies:
**narrow the justification, name the residue, file it, do not force it.**

The residue, as the exit condition's amendment: `send-seam(Phase 9)` retires when
`SVAAFrameInfo` (X), `SWelsSvcCodingParam` (Phase 8), `SScreenBlockFeatureStorage`
(Phase 10), `SLogContext` and `CWelsPreProcess` are raw-free — **it is a phase-exit
item, not a session item**, and the seam's comment should say so instead of naming
"Phase 9's context split", which is this session and is not sufficient.

Note also that "five of them inside types Phases 8 and 10 own" was already an
undercount when the seam's comment was written, and the brief carried it forward.
The measurement is eight not-G's, and the probe that settles it costs one build.

## F165 — the brief's accessor inventory is bounded by a one-file grep, and the flip it stages is decided by a family 29% larger: 27 accessor bodies, 10 of them ST

Session H's brief makes the accessor family the thing that decides the flip:

> **The accessors decide the stages.** 21 `ctx_*` accessors
> (`grep -c 'pub unsafe fn ctx_' src/encoder/encoder_context.rs`): **14 are
> in-fork** … and **7 are ST bodies** (`ctx_mb_index_x/y`, `ctx_ltr`,
> `ctx_ltr_at`, `ctx_frame_bs`, `ctx_frame_bs_cur`, `ctx_dq_idc_map`) …

The grep is correct and its number is correct — for `encoder_context.rs`. The
family is not in `encoder_context.rs`.

```
$ grep -c 'pub unsafe fn ctx_' src/encoder/encoder_context.rs      # the brief's grep
21
$ grep -rn 'pub unsafe fn ctx_' src/encoder/ | wc -l               # the relationship
25
```

The four the file boundary hides are in `svc_encode_slice.rs`: `ctx_sps:849`,
`ctx_pps:868`, `ctx_ref_pic:888`, `ctx_pic_ref:914` — and beside them live
`current_layer:713` and `set_current_layer:746`, the same class of body (a
cursor accessor on the context, `unsafe-cat: cursor`, minting a raw derivation
from `*mut sWelsEncCtx`) under a name that does not begin with `ctx_`. The
family is **27 bodies**, and the split the tool gives is **17 in-fork / 10 ST**,
not 14 / 7:

```
$ python3 rust/tools/phase9_forksplit.py --list      # columns; both re-derived at HEAD
ST accessors  (10): ctx_mb_index_x, ctx_mb_index_y, ctx_ltr, ctx_ltr_at,
                    ctx_frame_bs, ctx_frame_bs_cur, ctx_dq_idc_map,
                    ctx_sps, ctx_pps, set_current_layer
in-fork       (17): the brief's 14, plus ctx_ref_pic, ctx_pic_ref, current_layer
```

**The brief's own crux sentence names a body its own inventory cannot see.**
"`ctx_ltr_at` and `ctx_sps` are themselves ST bodies" is true of both; only
`ctx_ltr_at` is in the 21.

### Why the three missing ST accessors are not three more of the same

Had the residue been three more direct field projections, this would be an
arithmetic correction. It is not — each of the three is a shape the brief's
model has no slot for.

**1. `ctx_sps` and `ctx_pps` are chained accessors whose callee is in the other
column.** `ctx_sps` resolves `iSps` against `ctx_sps_array(pCtx)`, and
`ctx_sps_array` is **in-fork** — permanently raw under S63. So "the ST accessors
dissolve into direct field paths" cannot be executed on these two: the field
path runs *through* a body that is not allowed to take `&mut`. They dissolve
into a field path plus a raw hop, or they stay. `ctx_pps`/`ctx_pps_array` is the
same pair. 20 of the 90 ST-accessor call sites are these two.

**2. `set_current_layer` is the writer of a getter/setter pair split across the
columns.** `current_layer` is not merely in-fork — it is called *directly from a
`thread::scope` spawn closure*:

```
$ python3 rust/tools/phase9_forksplit.py --why current_layer
current_layer <- fork seed (thread::scope spawn)
$ python3 rust/tools/phase9_forksplit.py --why set_current_layer
set_current_layer is NOT fork-reachable
```

One field, `iCurDqLayer`; its only writer flips, its reader stays raw forever.
The brief's two-bucket model ("7 dissolve, 14 stay raw") has no way to state
that, and the flip has to: after `set_current_layer` becomes
`ctx.iCurDqLayer = …`, `current_layer` still reads that field through
`*mut sWelsEncCtx` from inside N workers. That read is a **seam question**
(step 3's surface), not a flip question, and the brief's step 3 does not carry
it because its step 2 could not see it.

### The second consequence: the tag harvest is over-claimed by 16

> `encoder_context.rs`'s 23 `cursor` tags fall with their accessors

Of the 23, **7** sit on ST accessors and fall with the flip; **13** sit on the
in-fork accessors S63 keeps raw permanently and cannot fall while the fork
exists; **3** are in the test module. Measured by walking each tag to the item
it precedes:

```
ST-accessor  7   ctx_mb_index_x, ctx_ltr, ctx_ltr_at, ctx_frame_bs,
                 ctx_frame_bs_cur, ctx_dq_idc_map, ctx_mb_index_y
in-fork     13   ctx_stride_dec/enc_block_offset, ctx_param, ctx_func_list,
                 ctx_mvd_cost_table/origin, ctx_dq_layer, ctx_ref_list,
                 ctx_rc, ctx_rc_at, ctx_sps_array, ctx_subset_array, ctx_pps_array
test         3
```

The same error in both places, and it is one error: the accessor family was
enumerated by a grep whose scope was a file.

### The class

This is **S64** exactly — "a grep bounds a spelling, not a relationship" — and it
is the rule's fifth instance in three sessions (F149, F154, F155, F156 were the
first four). S64's own remedy sentence is the one that was needed here: *"before
scoping a family, enumerate every spelling of the type's reach … a brief that
quotes one spelling is quoting a lower bound and says so."* The brief quoted a
lower bound and did not say so; it presented `21` as the family and built both
the stage plan and the harvest on it.

The narrower lesson worth carrying to J: **`encoder_context.rs` is not where the
context's accessors live, it is where most of them live** — and for a campaign
whose staging is decided by accessor coexistence, "most" is the wrong instrument.
Enumerate accessors by what they return from what they take
(`grep -rn 'unsafe fn \w*(\w*: \*mut sWelsEncCtx' src/`), not by the file that
usually holds them.

## F166 — the ST column is a candidate list, not the flip list: `ParasetStrategy` is ST, and flipping it turns ten sound call sites into borrow errors

`phase9_forksplit.py` names its second column *"ST-FLIPPABLE (a root-down
campaign can convert)"*, and every brief since E2 has read it as the work list.
S63 is careful about the other direction — an in-fork body **cannot** take `&mut`
— and says nothing about this one, because at the time the question had not been
asked. Asked now, the answer is no: **fork-unreachability is what makes a flip
permissible, not what makes it possible.**

One body in the 155 demonstrates it.

```rust
// paraset_strategy.rs:937 — unsafe-cat: cursor
pub unsafe fn ParasetStrategy<'a>(
    pCtx: *mut sWelsEncCtx,
) -> &'a mut CWelsParametersetIdStrategyObj {
    (*ctx_func_list(pCtx)).pParametersetStrategy.as_deref_mut().expect(…)
}
```

`ParasetStrategy` is in the ST column (`--why`: not fork-reachable). It is also
an accessor returning `&'a mut` with an **unbound** lifetime — its own doc says
so: *"The unbound lifetime is the usual laundering this port does at a
raw-pointer boundary."* Ten of its twenty call sites hold that reference across a
second use of the context:

```
encoder_ext.rs:901   ParasetStrategy(*ppCtx).LoadPrevious(…  *ppCtx …)
encoder_ext.rs:926   ParasetStrategy(*ppCtx).GenerateNewSps(…  *ppCtx …)
encoder_ext.rs:951   ParasetStrategy(*ppCtx).InitPps(…  *ppCtx …)
encoder_ext.rs:996   ParasetStrategy(*ppCtx).UpdateParaSetNum(*ppCtx)
encoder_ext.rs:2705  ParasetStrategy(pCtx).UpdatePpsList(…  pCtx …)
wels_encoder_ext.rs:499/520/566   ParasetStrategy(pCtx).Update(…  pCtx …)
wels_encoder_ext.rs:562  ParasetStrategy(pCtx).UpdatePpsList(…  pCtx …)
wels_encoder_ext.rs:856  ParasetStrategy(pCtx).OutputCurrentStructure(…  pCtx …)
```

Flip `ParasetStrategy` to `pCtx: &mut sWelsEncCtx` and the returned reference
becomes a borrow **of** the context; all ten become E0499, and none has a local
fix — the receiver and the argument are the same object by construction.

**Verified, not predicted** (a standalone `rustc` model of the shape, run
before this finding was written — S60; reproduced in `phase9_ctx_flip_plan.md`'s
appendix). The raw form compiles; the flipped
form is `error[E0499]: cannot borrow *ctx as mutable more than once at a time`.
The case worth recording is the second one: **keeping the method's parameter raw
does not rescue the site.** `strategy_ref(ctx).update_paraset_num(ctx as *mut Ctx)`
is E0499 too — the coercion is itself a use of `ctx`, and two-phase borrows do
not reach it because the receiver is a function's return value, not an autoref.
So "pass it raw at the boundary", which is the remedy every other collision in
this campaign takes, is exactly the remedy that fails here.

**And the sites are sound today, for the F163 reason.** `pParametersetStrategy`
is an `Option<Box<…>>`; `as_deref_mut()` lands in the *Box's* allocation, not the
context's, so a `&mut sWelsEncCtx` entry retag never reaches it. The laundering
here is not J's ghost — it is the same allocation argument that exempts
`pParamD`. Flipping this body would convert ten sound sites into ten broken ones
and buy nothing.

So `ParasetStrategy` stays `*mut sWelsEncCtx` permanently, for a reason unrelated
to the fork, and the flip list is **154**, not 155.

**And keeping it raw is not damage control — it is what unblocks the ten sites.**
With the accessor raw, the *methods* on the strategy object flip freely:
`strategy_raw(ctx as *mut Ctx).output_current_structure(ctx)` compiles clean
(appendix, `phase9_ctx_flip_plan.md`), because the receiver's lifetime is unbound and holds no
borrow of `ctx`. That matters immediately: `OutputCurrentStructure` is itself one
of the twelve depth-0 roots of the flip (`wels_encoder_ext.rs:856`), so stage A0
walks straight into this shape. One body staying raw is what lets the ten sites —
and the roots reached through them — flip at all.

### The rule this wants

An ST body must **also** stay raw when it returns a context-derived reference
whose callers hold it across a use of the context, *and* the referent is in
another allocation — because there the raw spelling is buying real coexistence,
not hiding a hazard. The test is mechanical and worth running before each stage
rather than discovering at `cargo build`: for every ST body returning a pointer
or a reference, check whether any caller passes the context alongside it.

Eleven ST bodies return a pointer or reference at HEAD. Nine are the ST
accessors phase B dissolves (F165); `CreatePreProcess` returns a newly allocated
object, not a context cursor; `ParasetStrategy` is the eleventh and the only one
this catches. **One instance is enough to make the column's label wrong**, and a
future family will have more — the phase-exit session should read
`forksplit`'s second column as "may flip, subject to a borrow check", and the
tool's own header should say it.

## F167 — the flip's root stage kills a stored raw copy of the context, and the detector cannot see it because it is a struct field, not a binding

`q1c.py` finds a hazard by looking for a **local binding** derived from the
context and held across a call that retags it. The context has one derivation
that is not a local binding and outlives every call:

```
$ # struct FIELDS whose type mentions sWelsEncCtx (brace-matched, all of src/)
src/encoder/slice_multi_threading.rs:1236  struct SliceJobHandle    pCtx: *mut sWelsEncCtx
src/encoder/wels_preprocess.rs:870         struct CWelsPreProcess   pub m_pEncCtx: *mut sWelsEncCtx
src/encoder/wels_encoder_ext.rs:1678       struct CWelsH264SVCEncoder  pub m_pEncContext: Option<Box<sWelsEncCtx>>
```

The third is the **owner**. The first is the fork's job handle — minted inside
the fork from the in-fork raw chain and dead at the join, and in-fork by
construction (S63). **The second is a cached copy stored at encoder-init time and
read for the whole life of the encoder**, and it is the one the flip breaks.

### Why it is sound today, and exactly what makes it so

`CreatePreProcess` stashes the pointer once (`wels_preprocess.rs:1001`,
`m_pEncCtx: pEncCtx`) and five sites read it back to re-derive the context —
`ctx_vaa(self.m_pEncCtx)` (1534, 2820), `(*self.m_pEncCtx).bCurFrameMarkedAsSceneLtr`
(2528), `ctx_param(self.m_pEncCtx)` (2819), `let pCtx = self.m_pEncCtx` (2249).
None of the five takes a context parameter; the field is their only route.

Today that is fine, and it is fine **for one specific reason** — the root
derivation is not a reference:

```rust
// wels_encoder_ext.rs:711 — T8.B5 (S42): "derived once, from the owner's `Box`."
let mut pCtx: *mut sWelsEncCtx = match ppCtx.as_mut() {
    Some(pEncContext) => std::ptr::addr_of_mut!(**pEncContext),
    None => return 1,
};
```

`addr_of_mut!` forms a raw place without an intermediate `&mut`. So `m_pEncCtx`
and every other raw derivation are *siblings* under the `Box`'s tag, and siblings
coexist. Nothing in the encoder ever mints a `&mut sWelsEncCtx` — the
measurement is flat zero:

```
$ grep -rn ': &mut sWelsEncCtx' src/encoder | grep -v ':\s*//' | wc -l
0
```

### What the flip does to it

Stage A0 replaces that `addr_of_mut!` with a `&mut` at the root, because that is
what "the roots take `&mut sWelsEncCtx`" means. The moment it does, a `Unique`
sits at the top of the context allocation's borrow stack and every raw sibling
derived earlier is popped — `m_pEncCtx` among them. The next
`ctx_param(self.m_pEncCtx)` is then a use of a dead tag.

**So the hazard is created by the flip, not inherited.** That is the unusual part
and the reason it deserves a number: every other item in this campaign is a
pre-existing hazard the flip *reveals*. This one is sound at HEAD, sound at the
end of phase A for every body that does not touch the preprocessor, and unsound
for exactly as long as stage A0 has landed and the remedy has not.

### Reachability — latent today, and that is not a defence

All five readers are screen-content paths (`GetBestRefPicScreen`,
`DetectSceneChangeScreen`, `GetAvailableRefListLosslessScreenRefSelection`,
`GetRefFrameInfo`), and `SCREEN_CONTENT` is dormant — which is why no probe has
ever driven them. But the call is in the tree on a live edge:

```
ref_list_mgr_svc.rs:1464   in WelsBuildRefListScreen()   (ST-flippable, depth 8)
    iLtrRefIdx = (*(*pCtx).pVpp).GetRefFrameInfo(…)
```

`WelsBuildRefListScreen` is one of the 155. After its stage it holds
`&mut sWelsEncCtx` and calls a method whose only route to the context is the dead
field. A dormant path is a path a gate does not cover, so this would land green
and stay green until someone enabled screen content.

### The remedy, and why it is stage A0's and not a later stage's

Three options, in the order they were considered:

1. **Re-stamp the field** at each entry to the frame loop
   (`self.m_pEncCtx = ctx as *mut _`), making it a fresh child of the live
   `&mut`. Cheapest, and it is the port's existing "re-derive per use" idiom
   (F71/S40) — but it leaves a raw alias of a `&mut`-governed object in a field,
   which is the shape this phase is retiring.
2. **Pass the context** to the five readers and delete the field. Correct, and
   the largest edit: four of the five are methods reached from
   `wels_preprocess.rs`-internal call chains that would each grow a parameter.
3. **Keep the root raw** and flip only below it — abandons the flip's own premise.

**(2) is the right end state and (1) is the right stage-A0 move**, because the
field must stop being a dangling tag in the same commit that mints the first
`&mut`, and (2) is a bigger change than a root stage should carry. Recorded so
the two commits are visibly one decision: A0 re-stamps, and a named later stage
deletes the field. If A0 lands without either, phase A is unsound from that
commit forward on a path no gate drives.

### Owed verification

The Stacked Borrows reasoning above is argued, not run — the fork pair was
holding the machine and a second Miri process would have corrupted its wall-time
(S20, one battery at a time). **A probe that drives the preprocessor's cached
read with a live `&mut sWelsEncCtx` is owed before stage A0 is called green**,
and its expected first verdict is stated here so it can be checked rather than
interpreted (D-gate/S60's C2 clause): *"use of a dead tag / attempting a read
access with tag … but that tag does not exist in the borrow stack"* against the
context's allocation, at `ctx_param` inside `GetRefFrameInfo`.
## F168 — E8's fork-probe numbers were parallel, not serial; the brief doubled the pair's cost by re-running it the wrong way, and G's stopped pair was about a minute from green

Session H's brief charters step 0a like this:

> `MIRI_FULL=1`, per-probe invocations (D-gate-7), **one probe at a time** —
> E8's reference numbers (3356 s / 3449 s) were *serial* numbers; run them the
> same way and expect ~115 min total.

They were not serial numbers. T9.E8's own log entry says so in the same sentence
that reports them:

> **Both fork/join probes ran green under Miri for the first time in the
> project's history** (T9.E8): the fixed-slice probe in **3356 s**, the mid-row
> probe in **3449 s** (*~57 minutes as a parallel pair*)

The parenthesis is decisive on arithmetic alone: 3356 + 3449 = 6805 s = 113
minutes. "~57 minutes" is the *longer of the two*, which is only the pair's wall
if they ran concurrently. E2's later run says the same thing in the same words —
"fixed-slice 3294.86 s PASS, mid-row 3343.20 s PASS (**~56 min as a parallel
pair**)".

### The measurement that settles it

This session ran the fixed-slice probe **serially, alone, on an idle machine**:

```
FIXED rc=0 wall=3199s      87 progress reports, 0 UB      (H step 0a, serial)
```

Against 3356 s (E8, parallel) and 3294.86 s (E2, parallel). **Serial buys 3%
per probe.** The machine is 8 logical cores and Miri is a single-threaded
interpreter — it schedules the interpreted threads itself rather than using host
cores — so two probes are two cores of eight and they do not contend.

So the pair's honest cost is:

| | wall |
|---|---:|
| parallel (E8, E2, and G's stopped run) | **~57 min** |
| serial (this brief's instruction) | **~111 min** |

The brief did not trade wall-time for anything. Both forms run the same two
probes over the same two drives and prove the same thing; one takes twice as
long.

### What this does to G's close

G's pair was stopped at **56 minutes** with both probes healthy and 68/65
progress reports, and G recorded them *incomplete, not green* — correctly, since
a killed probe proves nothing. But G's diagnosis of *why* was built on the same
misreading:

> "T9.E8's 3356 s / 3449 s were measured serially, so the parallel pair on a
> machine that had just run the session gate was always going to be slower than
> the ~57 min the brief budgeted. **The estimate, not the tree, is what was
> wrong.**"

The estimate was right. Against reference completions of ~3300–3450 s, a
parallel pair at 3360 s was **inside a minute or two of finishing**. G stopped at
the finish line and then wrote down a reason why the finish line had never been
reachable — and H's brief inherited that reason as an instruction, which is
F153's class again (a conclusion inheriting an earlier document's frame rather
than re-deriving it) one level up: not a stale date this time, a stale
*explanation*.

### Consequences

1. **The closing pair runs parallel.** Two per-probe invocations with separate
   `CARGO_TARGET_DIR`s (cargo's file lock serialises them otherwise — G's own
   note records this correctly), which is exactly what D-gate-7 asks for and
   what E8 and E2 both did. ~57 min, not ~111.
2. **The Miri baseline note should carry the pair's form, not just its number.**
   `miri_wall_baseline.txt` records the session lane's wall and says
   lane-vs-lane; the pair has no such line and its two regimes are 2× apart, so
   a future session comparing 57 to 111 would read a catastrophic regression
   that is only a scheduling choice. Recorded as a phase-exit item for J.
3. **S24's clause wants one more word.** A number is falsifiable when the
   command that produced it stands beside it. Two of these numbers came with
   their command and still misled, because the command was per-probe and the
   fact that mattered was *how the two invocations were scheduled relative to
   each other*. For a measured wall-time the unit is not the command, it is the
   command plus what else was running.

## F169 — the flip's boundary spelling is a ratchet question, and `as *mut _` is the wrong answer by about 500 counts

Phase A's whole mechanism is that a flipped body reaches a still-raw callee by
coercing: the context is `&mut sWelsEncCtx`, the callee takes
`*mut sWelsEncCtx`, and something must spell the conversion. Stage A0 wrote the
obvious thing at 93 sites in one body:

```rust
ctx_param(pCtx as *mut _)
```

`gates.sh family` came back **FAIL — unsafe ratchet: a file x metric increased**:

```
encoder/encoder_ext.rs  raw_ptr: 95 -> 187
```

`raw_ptr` counts occurrences of `*mut ` and `*const ` (`unsafe_ratchet.sh:41`),
so each coercion is a raw-pointer mention. One body cost +92.

**Extrapolated, the naive spelling would have added roughly 500 `raw_ptr` counts
across the campaign** — 94 boundary calls in this body against 517 derefs
overall, and every one of the 154 bodies has boundaries. The phase's headline
metric is 1587; a flip whose entire purpose is retiring raw pointers would have
finished about a third *above* where it started, and the ratchet — which has no
escape hatch, "decreases are always fine, `check` is a ratchet, not an equality
test" — would have failed on every stage.

The fix is a spelling, and it is the standard library's own name for the
operation:

```rust
ctx_param(std::ptr::from_mut(pCtx))
```

`std::ptr::from_mut::<T>(&mut T) -> *mut T` contains no `*mut ` token, so it is
**net zero** on the metric. Re-spelling the 93 sites turned the stage's totals
from +92 into:

```
raw_ptr        1587 -> 1584   -3
unsafe_block    282 ->  279   -3
unsafe_fn       626 ->  623   -3
no per-file increases vs baseline.
```

### Why this is not just bookkeeping

`as *mut _` and `std::ptr::from_mut` compile identically, so a reader could call
this cosmetic. It is not, for two reasons.

**It is the difference between a measurable campaign and an unmeasurable one.**
The ratchet is the only instrument that sees "unsafe fn bodies converted" (its
own header explains why `grep -c unsafe` cannot). A boundary spelling that
inflates the metric by one per call site drowns the signal in boundary noise:
the phase would have had to either abandon its instrument for the duration of
its largest campaign, or re-baseline every stage, which is the same thing.

**`from_mut` also says the right thing.** `as *mut _` is an inferred cast that
would silently accept a `*const`-to-`*mut` change or a different pointee if the
surrounding types moved; `from_mut` takes a `&mut T` and returns `*mut T` and
nothing else, so the boundary is type-checked as the reborrow it is. For a
campaign whose failure mode is minting the wrong pointer at a boundary, that is
worth having.

### The rule

**Every phase-A boundary is `std::ptr::from_mut(ctx)`, and no stage writes
`as *mut _`.** It is greppable, which the flip's remaining 153 bodies will want:
`grep -rn 'from_mut(pCtx\|from_mut(pEncCtx' src/encoder | wc -l` counts the
campaign's outstanding boundaries directly, and that number should fall to zero
as phase B dissolves the accessors and the in-fork half is enumerated at the
exit. A count of `as *mut _` would have been indistinguishable from the port's
pre-existing casts.

Recorded as a stage-0 lesson rather than a stage-A0 detail because it binds
every remaining stage, and because it is the second time this session that the
cheapest correct-looking spelling was the wrong one (F168's serial pair was the
first): **the tree's own instruments are the check on that, and they only work
if a stage runs them before it believes itself finished.**

### Amendment (T9.H4, the same session): the right answer is *no* explicit coercion

F169 was written after stage A0 and prescribed `std::ptr::from_mut(pCtx)` at
every phase-A boundary. Stage A1 falsified the premise both spellings rest on.

**Rust coerces `&mut T` to `*mut T` implicitly in argument position.** A flipped
body calling a still-raw callee needs no coercion at all:

```rust
ctx_param(pCtx)          // pCtx: &mut sWelsEncCtx, ctx_param: *mut sWelsEncCtx
```

This was discovered by accident and is worth recording as such: stage A1's
boundary pass silently failed to run, and the stage **compiled anyway** — 36
bodies flipped, and the only errors were the ten `is_null()` guards. Five files
had zero coercions written and zero type errors. A pass that does nothing and a
pass that does the right thing are indistinguishable from the exit code, which is
S66's shape one level over: the boundary pass had never been shown to be *load-
bearing*, only to be *green*.

**So the campaign takes the implicit form, and the tree is uniform on it.** A0's
93 explicit coercions were converted back. Three consequences, in the order they
matter:

1. **Each stage touches only the bodies it flips**, never their call sites. For
   154 bodies with ~500 boundaries that is the difference between a stage being a
   signature-and-derefs edit and a stage being a whole-tree edit. It also removes
   the un-spelling step: an explicit wrapper at a call site has to be *removed*
   when the callee flips later, so every boundary would have been written once
   and deleted once.
2. **The ratchet finding stands and is unchanged.** `as *mut _` costs one
   `raw_ptr` per site and would have added ~500 to the metric the phase exists to
   reduce. That was and is real; the correction is only that the remedy is
   cheaper than the one first proposed. A1's totals: `raw_ptr 1587 -> 1549`
   (**-38**), no per-file increases.
3. **The greppability argument F169 made for `from_mut` is withdrawn**, and it
   was the weaker half of the finding anyway. The campaign's progress metric is
   the census — `phase9_forksplit.py` reads 265/111/154 after A0 and moves with
   every stage — not a grep for a boundary spelling. Counting boundaries would
   have measured the flip's *scaffolding*; the census measures the flip.

**Explicit coercion is still required outside argument position**, and the tree
has exactly one such site: F167's re-stamp,
`(*pCtx.pVpp).m_pEncCtx = std::ptr::from_mut(pCtx);` — an assignment, where no
coercion site exists. That one keeps the explicit spelling, and it is the right
one to keep visible, because it is the campaign's only deliberate raw alias of
the root borrow.

### A second lesson from the same stage, about blanket edits

The ten `is_null()` guards look like one shape and invite one regex. Applying
`if X.is_null() || ` -> `if ` across `src/encoder/` trimmed **51** guards — the
ten that the compiler had flagged, and **41 in bodies that are still raw, where
the check is live and necessary**. The build would have stayed green (a raw
pointer's `is_null()` compiles fine either way) and the byte gates would have
passed, because no gate in this project drives a null context.

That is the whole failure mode this phase is built to avoid: a mechanical edit
that the compiler cannot referee and the gates cannot see. **The rule is that a
phase-A stage edits only what the compiler names**, one site at a time, and never
by pattern across a file the stage does not own — the compiler's error list *is*
the work list, and its length is the check on the edit's scope. The stage was
reverted whole and redone from the error list; the second attempt produced ten
edits and zero collateral.

## F170 — the gate battery times its sweeps with wall clock, so an unattended run reports machine sleep as compute

`gates.sh family` on stage A1 reported:

```
PASS  sweep (debug):   PASS=583 FAIL=0, 5887s wall
PASS  sweep (release): PASS=583 FAIL=0,   40s wall
```

5887 s is 98 minutes, against 46 s and 47 s on the two family gates earlier in
the same session — a 125x regression in one profile, on a stage that had just
flipped 36 bodies including both fork drivers. It read exactly like the flip
having made the debug encoder pathologically slow.

**It had not.** Re-run immediately, on the same tree, unchanged:

```
$ /usr/bin/time -p env RUST_ENC_PROFILE=debug \
      bash rust/tools/diffharness/sweep.sh st mt def sl ltr ps dl bg
PASS=583 FAIL=0
real 44.83
```

Per preset: st 12.2, mt 6.6, def 2.8, sl 1.5, ltr 2.8, ps 5.4, dl 6.3, bg 7.3.
No preset is slow, `mt` — the one the flip's fork drivers live in — least of all.

**The cause is the instrument.** `run_sweep` brackets the sweep with
`t0=$(date +%s)` / `t1=$(date +%s)` (`gates.sh:361,379`) and reports `t1-t0`.
That is wall clock, and wall clock counts suspend. The gate was launched, exceeded
its foreground budget, and continued unattended; the log mtimes bound it exactly —
`build_debug.log` 18:22:39, `sweep_debug.log` 20:00:46 — across a stretch with no
commands issued, during which the machine idled. The release sweep ran after it
woke and read 40 s, which is why only one profile looked broken.

### Why this is worth a number rather than a shrug

S61 exists because "a 2.6x Miri regression passed eight commits because nothing
watches the instrument". The same clause makes the gate's own cost a gated
quantity — and a cost measured in wall clock fails in **both** directions:

* **False positive**, which is what happened here: an idle period is indicted as
  a regression, and the session spends its time bisecting a stage that is fine.
* **False negative**, which is worse and silent: a genuine 2x slowdown inside a
  gate that also happened to run while the machine was busy with something else
  is indistinguishable from this. The number cannot be compared across runs
  unless every run had the machine to itself, and nothing checks that.

This is F168's shape in a different instrument, and the pair of them is the
general statement: **for a measured time, the unit is not the command — it is the
command plus everything else that was true while it ran.** F168's fork pair
recorded a per-probe wall whose meaning depended on whether the other probe was
running; this records a sweep wall whose meaning depends on whether the machine
was awake.

### The fix

Report CPU time, not wall clock. The shell already has it: bash's `time` keyword
and `times` both give user+sys for the subshell, and `/usr/bin/time -p` prints
`user`/`sys` beside `real`. Suspend does not accrue CPU time, and a busy machine
inflates `real` while leaving `user+sys` roughly honest — which is the property a
regression tripwire needs. **Report both**: `real` stays useful for a human
watching a session, `user+sys` is the number a threshold may be set on.
The same correction applies to `s61_report`'s Miri wall, which is `date +%s`
arithmetic too (`gates.sh:136,168`) and is the number the 1.3x tripwire actually
fires on — so today that tripwire can be tripped by an idle laptop. Left as a
phase-exit item for J rather than changed mid-session under a running campaign,
because changing the instrument and the tree in one session is what D-gate-6's
regime note warns against; recorded here so J does not have to rediscover it.

## F171 — the flip's hazard surface is larger than the parameter family, and that is why no instrument saw the UB it created

Session H's close ran `MIRI_SCOPE=encoder gates.sh session` and it **failed on
its first run**, two shards, one site, genuine Undefined Behavior created by this
session's own work:

```
error: Undefined Behavior: attempting a read access using <4020398> at
alloc276218[0x7b0], but that tag does not exist in the borrow stack
  --> wels_encoder_ext.rs:2180   pStatistics.iTotalEncodedBytes += ...
<4020398> was created by a Unique retag at offsets [0x770..0x7c8]
  --> wels_encoder_ext.rs:2126   &mut (*pCtx).sEncoderStatistics[iDid as usize]
<4020398> was later invalidated at offsets [0x0..0x17f10] by a Unique retag
  --> wels_encoder_ext.rs:2175   let pLtr = ctx_ltr(&mut *pCtx);
```

Read the three offsets together and the whole thing is there. `pStatistics` is a
`&mut` into the context's own allocation at `[0x770..0x7c8]`. `ctx_ltr` took
`*mut sWelsEncCtx` until T9.H12 and takes `&mut` now, so its call site became
`ctx_ltr(&mut *pCtx)` — a `Unique` retag over `[0x0..0x17f10]`, the whole
context, which contains and therefore pops `pStatistics`. The next use of
`pStatistics` reads a tag that is gone.

**This is exactly the shape `q1c.py` was built to find** — F66's shape A, a
cursor held across a call that retags. It is the shape the join tracked all
session. And the join read **0 LIVE from T9.H5 onward**, through six more
stages, while this was being created.

### Why both instruments were blind to it

`q1c`'s family, and `forksplit`'s, is *bodies that take a `*mut sWelsEncCtx`
**parameter***. `UpdateStatistics` is a method on `CWelsH264SVCEncoder`. It has
no context parameter at all — it holds the context in a **local** raw obtained
from `Self::ctx_ptr(&mut self.m_pEncContext)`. So it was never in the domain
either instrument scans, and no amount of driving the detector to zero could
have reported it.

That is the same hole F167 found from the other side. F167 named a **stored**
copy — `CWelsPreProcess::m_pEncCtx`, a struct field — and predicted the flip
would kill it, noting q1c cannot see it "because it scans local bindings and this
is a struct field". The symmetric case is a **local binding in a body outside
the family**, and it is the one that actually fired. The difference in how they
surfaced is instructive: F167's site is on a dormant screen-content path no gate
drives and remains argued rather than run; this one is on the main encode path
and took eleven minutes of Miri to find.

### The correction to the phase's model

The flip's precondition has been stated all session as "no live hazards in the
family". It is not. It is:

> no live hazard **anywhere the context is reachable by a `&mut` retag** —
> which is every body that can name the context, not every body that takes it as
> a parameter.

The parameter family is a *convenient* domain, not the *correct* one; it was
chosen because `S63`'s fork-split question really is about parameters, and the
hazard question was allowed to inherit that domain without anyone checking that
it fits. Enumerating the correct set means finding every route to the context —
parameters, locals from `ctx_ptr`, struct fields (F167's one), and anything
reached through `self` on the two encoder objects. That is a J-sized job and it
should be scoped as one.

**The standing guard in the meantime is the one that just worked.** D-gate-4 puts
`gates.sh session`'s Miri lane at every session close precisely because "the byte
gates passed 535/535 and only Miri saw it" (F114). It has now done that twice.
Nine stages of the flip passed `gates.sh family` at 583/583 in both profiles with
this UB live in the tree from T9.H12 onward, and no byte gate could ever have
seen it: the read returns the right value, it is simply reading through a pointer
Stacked Borrows has invalidated.

### The narrow lesson for whoever finishes the flip

Every remaining `&mut *pCtx` **written at a call site in a body that is not in
the family** is a candidate for this. They are greppable and there are few:

```
grep -rn '&mut \*pCtx\|&mut \*\*ppCtx\|&mut \*pEncCtx' src/encoder | wc -l
```

Each one retags the whole context. Before adding another, check what the
surrounding body is holding across it — the compiler will not, and neither will
the byte gates.
