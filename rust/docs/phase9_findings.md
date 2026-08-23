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
