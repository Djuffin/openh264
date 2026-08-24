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
