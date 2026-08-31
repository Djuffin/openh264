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

---

## F172 — F86's own diagnosis named a guarded site, and the brief inherited the misattribution

**Session X, step 1.** F86 (phase 8b) recorded an abort and attributed it to a
write the port has bound-guarded since the raw translation. Session X's brief
inherited that attribution and added an error of its own on top.

F86's text says the panic was the unchecked short-reference **insert** shift:

> **The C++ writes `pShortRefList[iRefIdx + 1]` unchecked** at
> `ref_list_mgr_svc.cpp:387–391`, so the same state corrupts memory there and
> panics here.

It did not. That shift is guarded in this port — `if iRefIdx + 1 <=
MAX_SHORT_REF_COUNT` — and has been since `68c4f6a5`, the original raw
translation, so it cannot raise an index panic at all. Checking out `6d682b7e`
(T8b.A3, the commit F86 names) and reading `ref_list_mgr_svc.rs:684` — the exact
line F86 quotes — gives

```rust
(*pRefList).pNextBuffer = (*pRefList).pShortRefList[lastIdx];
```

in `PrefetchNextBuffer`, with `lastIdx = uiShortRefCount - 1`. At
`uiShortRefCount == 6` that is index 5 on a `[_; 1 + MAX_SHORT_REF_COUNT]` =
`[_; 5]`, which reproduces F86's message exactly (`the len is 5 but the index is
5`). Its C++ counterpart is `ref_list_mgr_svc.cpp:343` — an unchecked **read**,
in a different function, in the opposite direction.

The brief then compounded it, pointing at a **third** site:

> **F86-open — the short-ref shift**: `ref_list_mgr_svc.rs:299–310` shifts
> `pShortRefList[k] = pShortRefList[k+1]` with safe indexing where the C++
> (`ref_list_mgr_svc.cpp:387–391`) writes unchecked.

`rs:299–310` is `DeleteSTRFromShortList`, the *delete* shift. `cpp:387–391` is
the *insert* shift in `WelsUpdateRefList`. They are not counterparts; they are
opposite operations in different functions.

**The class.** F153 says a brief inheriting a finding inherits the finding's
*date*. This is the same failure one level down: a brief inheriting a finding
inherits the finding's **attribution**, including the line number it got wrong,
and a citation that has been copied twice reads as twice-confirmed. The check
that catches it is cheap and nobody ran it for two sessions: check out the commit
the finding names and read the line it quotes.

**Two brief-level citations that no run of the tools produces.** The same
paragraph says the file has "zero in-fork bodies (the forksplit's table:
`ref_list_mgr_svc.rs` 0/32)". The forksplit's default table has no
`ref_list_mgr_svc.rs` row at all — its domain is the 114 bodies that still carry a
`*mut` sWelsEncCtx parameter, and post-H this file's bodies take `&mut`. Aimed at
the LTR types it prints `0 / 3`, not `0 / 32`; the file has 41 functions and 31
`unsafe fn`, so no run of any instrument produces a 32 either. The **conclusion**
is right — checked with `--why`, the walker's reachability arm, which is the arm
that answers the question the parameter table cannot.

**Resolution (not a ruling request).** Measured rather than argued: a probe on
`uiShortRefCount` at all three exposed sites, over the whole 583-row sweep, tops
out at **2** against a panic threshold of **6**, and `PrefetchNextBuffer`'s exposed
branch is never entered at all. F86's own trigger was `ForceCodingIDR` being a
stub, which T8b.A4 fixed. No reachable panic, so the brief's other branch applies:
the three exposed reads are guarded exactly where upstream's invariant is implicit,
byte-neutral wherever that invariant holds, with the invariant named at each site.

---

## F173 — the gtest gate has been aborting since some Phase 9 session, and no session-level gate runs it

**Session X, found while chasing F86.** `gtest_stretch.sh --check` does not
complete: the binary takes `Abort trap: 6` in
`EncodeDecodeTestAPI.SimulcastAVC_SPS_PPS_LISTING`, so the suite prints no
`[==========] n tests ran` line and the gate cannot tally at all. Phase 8b's close
recorded 191/199 with an allowlist of exactly 8, so it ran to completion then.

```
thread '<unnamed>' panicked at src/encoder/svc_mode_decision.rs:1477:52:
mb_xy 549 >= 549 (grid 61x9)
panic in a function that cannot unwind  ->  abort
```

Reproduced on a clean tree with this session's work-in-progress reverted, so it is
inherited, not caused here. The row is **not** on
`gtest_known_failures.txt`.

**It is F162's shape, live.** The site is `GetRefMb`:

```rust
*(*std::ptr::addr_of!((*kpRefLayer).sMbDataP)).get(kiRefMbIdx as usize)
```

introduced by `4ab1919a` (session E3), replacing a raw hand-out with checked
indexing. Upstream reads the same index unchecked and says why in its own comment:

```cpp
const int32_t kiRefMbIdx = (pCurMb->iMbY >> 1) * kpRefLayer->iMbWidth + (pCurMb->iMbX >> 1);
  //because current lower layer is half size on both vertical and horizontal
return (&kpRefLayer->sMbDataP[kiRefMbIdx]);
```

Simulcast with a layer pair that is not an exact 2:1 ratio breaks that implicit
invariant, so upstream reads out of bounds and the port panics — **a Phase 9
safety conversion turned upstream's silent OOB read into an abort**, which is
exactly the D-fid pattern the steward rules on.

Out of session X's lane (`svc_mode_decision.rs` is not in the family), so it is
filed rather than fixed. Two things follow for the phase:

1. **The `exit` gate is currently red and nothing below `exit` runs it.** Every
   session since E3 has been green at `family` with this in the tree. If the
   gtest suite is the referee for the API surface, a session-level `--check`
   (even filtered) is cheap insurance; at minimum the phase exit should not be
   the first place this is discovered.
2. It needs the same ruling F162 got: guard to match upstream's read, or accept
   the panic as a deliberate divergence.

---

## F174 — `SWelsSvcRc::pGomComplexity` is a second dead sibling, where the brief says the dead one was the only one

**Session X, step 2.** The brief describes the three GOM arrays behind `rc.rs`'s
raw accessors as "the three **live** GOM arrays (D-dead-3 deleted their dead
fourth sibling; these three are the mechanism with ten C++ uses)". Both halves are
wrong.

`SWelsSvcRc::pGomComplexity` (`rc.h:188`, `double*`) is **allocated
(`ratectl.cpp:73`), nulled (`:87`), and `memset` to zero (`:668`) — and never read
anywhere in the reference.** The port mirrors the memset faithfully
(`rc.rs:1550`) and likewise never reads it; its accessor `rc_gom_complexity` had
**no production caller at all**, only the sibling-derivation test. That is
D-dead-3's `pGomCost` exactly, a second time.

And "ten C++ uses" is `pTemporalOverRc`'s number — ten reads in `ratectl.cpp`,
which is why *that* root was worth retiring onto direct indexing. The GOM arrays'
real numbers: `pCurrentFrameGomSad` has two encoder-side reads (`:734`, `:740`,
both inside the in-fork `RcGomTargetBits`); `pGomForegroundBlockNum` has none —
the encoder only zeroes it and hands it to the VP.

The accessor is retired (S18, zero callers). **The field itself needs a ruling**,
because deleting it also deletes the `memset` the port faithfully mirrors, and
D-dead-3 and D-dead-5 were both ruled rather than assumed.

---

## F175 — a doubly-thresholded verdict swallows a plane-root fault, and the escalation is the measurement

**Session X, step 3a.** The scene-change detector's conversion was refereed with a
planted plane-root fault, per S55/S59. The honest sequence:

| fault | rows failed of 583 |
|---|---|
| current-luma plane view `origin + 1` | **0** |
| verdict forced to `LARGE_CHANGED_SCENE` | **71** |

The first reading is not "the path is uncovered". `Process` is entered in **101**
of the 583 configurations (counted by a probe in the same run) and the second
reading proves the sweep sees the verdict. The path is covered; the *fault* was
too weak, because the verdict is quantised twice: a block counts only if its SAD
exceeds `HIGH_MOTION_BLOCK_THRESHOLD` (320), and the frame's verdict changes only
if that count crosses 85% or 50% of the block total. Shifting every block by one
pixel moves neither threshold.

**S64/F155 one level deeper.** F155 says to perturb a field for its per-site
variation. This adds: when the observable is a threshold over a count of
thresholded quantities, per-site variation is not enough either — the fault has to
move the *verdict*. A 0-row result on such a site is only informative alongside a
maximal fault that is non-zero. Escalate before concluding, and report both.

---

## F176 — the read grep was six callers short, on the very family F119 was written about

**Session X, step 3b.** The brief says the five raw VAA kernels' "only remaining
callers are the file's own differential tests (`:755/:790/:845`)". There were
**nine** call sites. The other six are in `tests/kernels_differential_phase2.rs`
(`:560`, `:570`, `:578`, `:584`, `:591`, `:641`).

F119 exists because a dead-grep excluded `tests/` and the "dead" `mc.rs` shims
turned out to be that file's raw entry points. This is the same file, the same
omission, one session later. The rule is not new; what is new is that quoting
three specific line numbers made the count look *measured*. S24 says a number that
decides a conversion comes with the command beside it — and the command here would
have had to include `tests` and `benches`, which is F119's whole content.

Both of that file's VAA properties re-anchored on the safe kernels in the same
commit and neither weakened; see the commit for how the span property survives the
move from pointer to slice.

**A stale comment was the reason the shims looked load-bearing.** `Process`
carried: "The shims stay: they are the C-ABI-shaped subjects
`tests/kernels_differential_phase2.rs` runs against the reference implementation."
That harness has no C++ side, and **F124 corrected exactly this belief about
exactly this file**. A stale justification comment outlives the finding that
refutes it, because nothing greps comments for claims.

---

## F177 — three buffers the port never allocates, with four unguarded dereferences, all dark

**Session X, step 3c.** Converting `SVAAFrameInfo`'s raw members turned up not
dead fields but **missing ports**:

| field | upstream | this port |
|---|---|---|
| `sAdaptiveQuantParam.pMotionTextureUnit` | `WelsMallocz(iCountMaxMbNum * sizeof(SMotionTextureUnit))`, `encoder_ext.cpp:1721` | never allocated — permanently null |
| `sAdaptiveQuantParam.pMotionTextureIndexToDeltaQp` | `WelsMallocz(iCountMaxMbNum)`, `encoder_ext.cpp:1724` | never allocated — permanently null |
| `sComplexityAnalysisParam.uiRefMbType` | `SetRefMbType(pCtx, &.., pRefPicture->iPictureType)`, `wels_preprocess.cpp:916` | **`SetRefMbType` was never ported** — permanently null |

All three are dereferenced **unguarded**: `adaptive_quantization.rs` at four sites,
`complexity_analysis.rs:169/:184`. They have never fired because
`bEnableAdaptiveQuant = false` in both diffharness drivers (`cxx_enc.cpp:150`) and
the complexity readers sit behind the same family of flags — S57's "no gate runs
this", holding up a null dereference rather than merely an unconverted site.

The first two are fixed here: the constructor cuts both blocks, matching upstream,
and the buffers are owned `Vec`s on `SVAAFrameInfo` reaching the plugin as slices.
The third is **not**, and should not be by a safety session: porting a missing
function is a correctness job. It is why `*mut`-u32 is still one of
`SVAAFrameInfo`'s `!Sync` reasons.

**The general point.** A permanently-null field with live readers looks identical
to a dead field under every instrument this phase owns — the census sees a tagged
raw site, the ratchet sees a pointer, the byte gates see nothing. What
distinguishes them is one grep on the *other* tree for the writer. F156 said to
grep a cached copy's readers per field before re-routing it; this says to grep the
**writer**, in the reference, before believing a field is inert.

---

## F178 — the ratchet counts its own subject matter in prose

**Session X, steps 3c and 4.** `unsafe_ratchet.sh`'s metrics are textual counts
over the source, comments included. Documenting a raw pointer therefore raises
`raw_ptr`, and documenting the absence of an export attribute raises `no_mangle`.
Both were observed as **gate failures on documentation**:

* `gates.sh family` failed on `processing/complexity_analysis.rs raw_ptr: 0 -> 2`
  — both counts prose in the doc comment explaining that the raw pointers had left
  the file.
* `gates.sh commit` failed on `encoder/nal_encap.rs no_mangle: 0 -> 1` — the count
  was the literal token inside a comment stating that the function does not carry
  one.

Two of the eight metrics, demonstrated. The consequences are worth stating
separately because they point in opposite directions:

1. **A conversion is penalised for explaining itself.** The incentive is to
   convert quietly, which is the opposite of what every other rule in this phase
   asks for.
2. **The absolute numbers are slightly wrong in the sessions' favour or against
   it, unpredictably.** Respelling this session's prose to avoid the literal
   tokens took **4 counts** of already-committed prose out of `raw_ptr` — the
   close reads 1369 where steps 0–3b's own commit messages imply 1373.

The fix is small (skip comment lines, or count only outside them) and it is a
*ratchet* change, so it needs a rebaseline with the reason recorded — S24's
clause. Until then, per-file `raw_ptr` deltas of 1–2 on files whose comments
changed are noise, and sessions should say so when they quote them.

---

## F179 — `SetRefMbType` has been ported since 2026-08-06; F177's third row, the port's own comment, and X2's whole step 3 all say otherwise

**Session X2, step 3, before writing a line.** S68 says to check out the commit a
finding names and read the line it quotes. F177's third row says:

| field | upstream | this port |
|---|---|---|
| `sComplexityAnalysisParam.uiRefMbType` | `SetRefMbType(pCtx, &.., pRefPicture->iPictureType)`, `wels_preprocess.cpp:916` | **`SetRefMbType` was never ported** — permanently null |

**It was ported in `580c678d`, 2026-08-06 — 806 commits before this session's
HEAD, and before Phase 9 began.** It is `wels_preprocess.rs:2696-2733`, a faithful
transcription of `wels_preprocess.cpp:811-835` down to the two
`*pRefMbTypeArray = pRef->uiRefMbType` assignments, and it is *called* at
`wels_preprocess.rs:2825`, which is upstream's `:916`. `80f9081e` (session H)
retyped its first parameter, so Phase 9 has edited the function it is said not to
have.

The port's own doc comment asserts the same falsehood, 2,250 lines above the
function, in the same file:

> `pBackgroundMbFlag` and `uiRefMbType` stay: ... the second has **no writer in
> this port at all** — upstream fills it through `SetRefMbType`
> (`wels_preprocess.cpp:916`), which was never ported.

That comment is `wels_preprocess.rs:443-447`, written by session X. F177 took it
at face value; X2's brief took F177 at face value and built a step on it ("port
the 25 lines and restore the field's buffer the way X restored its two
siblings"), with an expected close verdict of F67 ten → nine.

**Three things follow, and only the first is about this field.**

1. **There is no buffer to restore, and there never was.** `SetRefMbType` takes
   `uint32_t**` and *assigns a pointer*: it aims the VAA's field at
   `SPicture::uiRefMbType`, an array the picture pool owns and
   `picture_handle.cpp:98` allocates. The two siblings X did restore
   (`pMotionTextureUnit`, `pMotionTextureIndexToDeltaQp`) were `WelsMallocz`'d
   blocks belonging to `SVAAFrameInfo` itself. The brief generalised from two
   allocations to a third that is an alias, and F177's own table says "`SetRefMbType
   (pCtx, &.., ...)`" — the `&` was the tell.

2. **The readers are not dark.** F177 says all three fields sit behind
   `bEnableAdaptiveQuant = false`. That is true of the first two; `uiRefMbType`'s
   readers are behind `AnalyzePictureComplexity`'s **RC-mode** gate — `iRCMode` in
   {QUALITY, BITRATE, TIMESTAMP} with the matching slice type — and `sweep.sh`
   runs `RCMODES=(-1 0 1 2 3)`. The path has byte coverage today, which is *why*
   the field is written and read correctly and why 583/583 holds.

3. **F67's `*mut u32` cannot fall by handing over a slice, and the reason is
   upstream's stickiness.** `SComplexityAnalysisParam` is a persistent member of
   `SVAAFrameInfo`, and `SetRefMbType` is called only `if (pRefPicture)` — and,
   inside, assigns only if a reference in the list qualifies. On a frame with no
   usable reference the field **keeps the previous frame's pointer** and the GOM_VAR
   reader dereferences it. Passing `Option<&[u32]>` computed per call, the way T9.X
   passed the two GOM arrays, would send `None` exactly where upstream reads stale
   data, which is a behaviour change no byte gate on the `bg` preset would let
   through quietly. Storing a persistent `SrcPicId` instead is not equivalent
   either: pictures are recycled, so a stale id resolves to a *different* buffer
   than a stale pointer does. This is a real conversion with a real hazard, not
   the twenty-five lines the brief costed it at.

**Expected ten, measured ten** (S60): the probe at the close returns the same
count as X's, because nothing in step 3 was ever going to move it.

**The general point, which is F86's and S68's, one level deeper.** F86 cost two
briefs because they inherited a citation. This cost a step because a *comment*
asserted a negative about the tree it lives in — and a negative about your own
tree is the cheapest claim in the world to check and the easiest to write without
checking. `grep -n 'fn SetRefMbType'` was the whole audit.

---

## F180 — `WelsUnloadNal`'s caller enumeration was four short, in a file nobody grepped

**Session X2, step 1.** X adjudicated `WelsUnloadNal` as not a C-ABI boundary and
recorded the evidence in the source:

> It carries no export attribute, it is installed into no dispatch slot, and its
> five callers are all Rust (`encoder_ext.rs:2254`, `:2263`, `:2318`, `:3161`,
> `:3606`).

There are **nine**. X's list misses `encoder_ext.rs:3814` and all three in
`wels_encoder_ext.rs` (`:404`, `:442`, `:541`) — a whole second file. The brief
repeats "five Rust callers" as X's evidence.

**The verdict is unaffected**, which is the point worth recording: all nine are
ordinary Rust calls, so `extern "C"` really was a vestige and X2 dropped it. But
the claim being carried was *"all its callers are Rust"*, and that is a universal
over a set the enumeration did not close. Had the tenth caller been an
`extern "C"` shim in the file that was not grepped, the conclusion would have been
wrong and the evidence would have looked exactly the same.

S64 already says to enumerate types outward. This says the same thing about
call sites: an enumeration that names a file is evidence about that file.

---

## F181 — `_pLogCtx` was not a dead parameter; eight `WelsLog` calls were missing, and the brief's instruction was to delete the evidence

**Session X2, step 1, `au_set.rs`.** The brief's instruction:

> the two `_pLogCtx: *mut SLogContext` parameters are unused by name — S54: read
> every caller, then **delete** the dead parameters rather than retype them.

Read the reference first and the underscore means the opposite of what it looks
like. `WelsBitRateVerification` (`encoder_ext.cpp:74`) logs through that context
**six** times; `WelsCheckNumRefSetting` (`au_set.cpp:88`) logs **twice**. Eight
messages, and every one of them narrates a parameter the function silently
*rewrites*: an invalid bitrate rejected, `iMaxSpatialBitrate` replaced from the
level table, `uiLevelIdc` moved under it, `iLTRRefNum` reset, `iNumRefFrame`
reset. The port performs every one of those rewrites and announced none of them.

Deleting the parameter would have removed the last trace of the omission and made
the gap unrecoverable without a diff against the reference.

**Restored in X2**, and the log referee measured the result: before this session
the port emitted **one** of the reference's nineteen messages on the fixed row;
after `TraceParamInfo` (F100) and these eight, it emits **nine, character for
character identical**.

**The rule.** F177 said: grep the reference's *writer* before believing a field is
inert. This is the same sentence with "parameter" for "field". An unused name is
evidence about the tree you are reading and about nothing else; whether it was
*supposed* to be used is a question only the other tree can answer. Three
consecutive sessions have now been caught by some form of this, on a field
(F177), on a name collision (D-dead-6's three `pGomComplexity`s) and on a
parameter (here).

---

## F182 — `TraceParamInfo` has four call sites upstream and had three here, and the port's own doc pointed at `GetOption`

**Session X2, step 2.** Two errors that survived because the function's body was
empty and nothing downstream could notice.

1. **A missing call site.** `welsEncoderExt.cpp` calls `TraceParamInfo` at `:202`,
   `:229`, `:334` **and `:796`** — the last inside
   `case ENCODER_OPTION_SVC_ENCODE_PARAM_EXT`, immediately after the parameter
   block is copied and *before* the spatial-layer check that may reject it, so a
   caller whose settings are about to be refused still gets them echoed. The port
   had the first three. X2's brief lists "`:202/:229/:334` — the init/SetOption
   paths", which is where the omission was inherited rather than found.
2. **A citation to the wrong function.** The port's doc comment said
   "`welsEncoderExt.cpp:1197` dumps the whole parameter block". `:1197` is inside
   `CWelsH264SVCEncoder::GetOption`. `TraceParamInfo` is at `:505`.

Both fixed here. Neither could have been caught by any gate in this repo before
`log_referee.sh` existed, which is the argument for it in one sentence: the
encoder had a *stub with three of its four callers wired to it and a doc pointing
at a different function*, and every gate was green.

---

## F183 — `pCurPath` is a third field of D-dead-3's shape: written in both trees, read in neither

**Session X2, step 1, `param_svc.rs`.** The file's one raw site is
`pCurPath: *mut c_char` (`param_svc.h:118` upstream), the library path a caller
hands in through `SetOption(ENCODER_OPTION_CURRENT_PATH)`.

The whole of `codec/` touches it three times: the declaration (`:118`), the null
in the constructor (`:228`) and the store in `welsEncoderExt.cpp:1076`. **No
read.** This port has the same three (`param_svc.rs:251`, `:309`, `:489` and
`wels_encoder_ext.rs:2489`) and likewise never reads it.

That is `pGomCost` (D-dead-3) and `pGomComplexity` (D-dead-6) a third time, and it
is **filed rather than executed** for two reasons. Each of those two was a
*ruling*, not an inference a session made on its own. And this one is not
identical to them: deleting it also deletes a store that sits behind a documented,
C-ABI-visible option id, so `SetOption(ENCODER_OPTION_CURRENT_PATH)` would go from
"stores a pointer nobody reads" to "succeeds and does nothing". Observably the
same, formally not — which is exactly the kind of distinction the steward has
been ruling on.

Meanwhile the field stays raw and X2 documented why: the value is a C string the
*caller* owns, stored verbatim as upstream stores it. An owned `CString` would
copy a buffer the reference does not copy. F166's shape — a permanent raw site
inside a file the session otherwise converts.

**And it is worth more than a tidy-up: `pCurPath` is one of F67's ten.** It is
`SWelsSvcCodingParam`'s *only* raw member, and `c_char` is `i8` on this target, so
it is exactly the `*mut i8` row F164 attributes to `SWelsSvcCodingParam` — the one
labelled "Phase 8's". Deleting it takes `sWelsEncCtx`'s `!Sync` count **ten to
nine**.

That is the number X2's brief predicted for this session, by a route that never
existed (step 3, F179). The route that does exist is a five-line deletion of a
field nobody reads, in a file the brief listed as `0 unsafe fn / 1 raw` and
expected to be trivia.

---

## F184 — the port's `WELS_LOG_*` levels are consecutive integers where the reference's are a bit mask, in five places, and the level reaches the caller's own callback

**Session X2, step 2 — found by `log_referee.sh` on its first run**, which is the
whole reason it was built.

`codec_app_def.h:323-331`:

```c
WELS_LOG_QUIET   = 0x00,      WELS_LOG_ERROR  = 1 << 0,   WELS_LOG_WARNING = 1 << 1,
WELS_LOG_INFO    = 1 << 2,    WELS_LOG_DEBUG  = 1 << 3,   WELS_LOG_DETAIL  = 1 << 4,
WELS_LOG_RESV    = 1 << 5,    WELS_LOG_DEFAULT = WELS_LOG_WARNING
```

The port (`common/wels_trace.rs:56-63`, citing that exact line range):

```rust
QUIET 0, ERROR 1, WARNING 2, INFO 3, DEBUG 4, DETAIL 5
```

`ERROR` and `WARNING` agree by coincidence — `1 << 0` and `1 << 1` are 1 and 2.
Everything above them diverges: **INFO is 4 upstream and 3 here, DEBUG is 8
upstream and 4 here**, so the port's `DEBUG` is bit-identical to the reference's
`INFO`. Measured, not reasoned: on the referee's fixed row the reference delivers
levels `{2, 4}` and the port delivers `{3}` for the same messages.

**It is C-ABI-visible in both directions.**

* The level is the **second argument of the caller's own trace callback**. An
  application that switches on `WELS_LOG_INFO` from the real header sees the
  port's DEBUG messages as INFO, and the port's INFO messages as a value the
  header does not define.
* It is the value `SetOption(ENCODER_OPTION_TRACE_CALLBACK, ...)`'s companion
  `ENCODER_OPTION_TRACE_LEVEL` is compared against, and the comparison is a
  threshold (`m_iTraceLevel < iLevel`, `welsCodecTrace.cpp:76`). Asking the
  reference for `4` admits ERROR, WARNING, INFO. Asking this port for `4` admits
  ERROR, WARNING, INFO **and DEBUG**, and not DETAIL. Different sets from the same
  documented request.

**The definitions are duplicated five times** and all five are wrong the same way:
`common/wels_trace.rs`, `decoder/decoder_core.rs`, `decoder/decode_slice.rs`,
`decoder/manage_dec_ref.rs`, and — the one that matters most —
`tests/trace_callback_test.rs:41`, which hardcodes `const WELS_LOG_INFO: i32 = 3`
and cites `codec_app_def.h:323`. **The test enshrines the wrong value**, so the
port has a green test asserting the divergence is correct.

**Why no gate has ever seen it.** The levels never touch a bitstream, so the
sweeps and the benches are blind; the ratchet counts pointers; Miri interprets
memory; `gtest_stretch.sh` links `test/api`, which does not assert on trace
levels. The only instrument that can see it is a second encoder logging the same
run, which is what the referee is.

**Not fixed here, and the reason is scope rather than difficulty.** The fix is
mechanical — the values are 1, 2, 4, 8, 16, 32, the `<` threshold keeps working
because they stay monotonic, and nothing does arithmetic on them. But it spans
**encoder and decoder**, it changes behaviour visible to any caller of the
cdylib, and this session's lane is the encoder's `other` family. Every ABI-visible
divergence in this phase has been a ruling (D-poc-1, D-fid-2, D-fid-3, D-scr-1).
This one wants the same, and it should land as one commit touching all five
definitions plus the test, with the referee's level check as its acceptance.

---

## F185 — the session battery is over D-gate-6's 15-minute cap, and the smoke is not why

**Session X2, the close.** D-gate-6 (the user, 2026-08-24) caps the whole
`session` level at fifteen minutes — "even if we need to reduce the amount of
tests that we run" — and the level was re-architected into a native lane and a
parallel Miri lane to meet it.

X2's close, measured end to end: **977 s = 16 min 17 s. Over the cap by 77 s.**

**The smoke this session added is not the cause.** It reports its own wall and it
was **9 s** against a self-enforced 60 s budget. 977 − 9 = 968 s, still over. The
battery was already past 900 s before D-fid-3 touched it.

Where the time goes, from the same run's own numbers:

| step | wall |
|---|---:|
| miri lane (parallel, 4 shards, encoder scope) | 480 s |
| sweep (debug) | 47 s |
| sweep (release) | 40 s |
| gtest simulcast smoke | 9 s |
| **the rest** — `cargo build --all-targets`, `cargo test` x2, ratchet, census | **~400 s** |

The Miri lane at 480 s is *below* the previous close's 507 s (S61 ratio 0.95), so
the regression is not there. It is the native lane, and specifically the two
`cargo test` runs: the same steps measured ~4 minutes in this session's earlier
`family` runs and visibly longer under the session level, because the Miri lane's
compile and five interpreters are taking cores from them. **The parallelism that
made the cap reachable is also what makes the native lane slower**, and D-gate-6's
own comment predicts the trade without pricing it.

Three honest options, none of them a session's to pick:

1. **Accept 16 minutes** and restate the cap. The number the ruling was reacting
   to was ~40 minutes; the level is now 40% over a round number rather than
   160% over.
2. **Shrink the native lane at `session`.** The obvious candidate is the debug
   `cargo test` run: the integration suite runs in both profiles and a type error
   is profile-independent (`gates.sh`'s own argument for why the `--all-targets`
   build is debug-only). Dropping *one* profile's integration tests at `session`
   would take roughly 150-200 s, and it is exactly the kind of coverage cut
   D-cov-1 says must be named out loud.
3. **Cap the lanes rather than the level**, so the number the ruling governs stops
   depending on how well two lanes share eight cores.

Filed rather than acted on: it is a change to the gate the user ruled on, and
this session's mandate for the gate was to *add* a smoke to it, not to re-cut it.

**Instrument caveat, inherited**: F170 already says `run_sweep` and `s61_report`
time with `date +%s`, so every number above is wall and not CPU, and an idle or
busy machine moves them. That fix is J's, and it matters more now that a ruling
turns on a wall-clock threshold.

---

## F186 — the referee's first covered run of `LogStatistics` found three defects, one of them in the referee itself

**Session X2, the follow-up.** `log_referee.sh` shipped with `LogStatistics`
ported and **zero coverage of it**: the capture contained no `EncoderStatistics:`
line on *either* side, so half of F100 was unverified and the script still said
`PASS` on the text it did compare. That is S57's shape aimed at a new instrument —
a gate whose green includes a region it never reaches.

**Why no config could fix it.** `LogStatistics` has exactly two callers.
`UpdateStatistics`'s needs `kiDeltaFrames > fMaxFrameRate * 2` *and*
`kiTimeDiff >= iStatisticsLogInterval` — tens of frames at 30 fps, which the
referee's row (and every sweep row) is far short of. The other two are
`SetOption(ENCODER_OPTION_SVC_ENCODE_PARAM_BASE / _EXT)`, and **neither
diffharness driver has ever called either option**: `grep -c 'SVC_ENCODE_PARAM'`
is 0 in both `cxx_enc.cpp` and `rust_enc/main.rs`. The coverage gap was in the
drivers, not the configuration.

Both drivers gained a 23rd argument that re-applies the *same* `SEncParamExt`
through the EXT arm after frame N-1 — a no-op for the encode, the only door in
for the trace. The referee's row uses it and the row stays byte-identical, which
is worth stating: the trace divergences below are about logging and not a symptom
of an encoding difference.

### 1. `%d` on an unsigned field prints the signed reinterpretation

The first covered run:

```
cxx :  uiIntraPeriod= -1
rust:  uiIntraPeriod= 4294967295
```

C's `%d` on a `uint32_t` reinterprets the bits; Rust's `{}` on a `u32` does not.
**Sixteen fields across both functions were wrong the day they were written** —
`uiIntraPeriod`, `iLtrMarkPeriod`, `uiMaxNalSize`, `iMultipleThreadIdc`, the two
`sSliceArgument` counts, and nine of `SEncoderStatistics`' unsigned members. Every
one is now cast to `i32` at the call, with a doc note saying why so a later reader
does not tidy them back.

The value is only visibly wrong when the field is large enough to set the sign
bit, which `uiIntraPeriod = 0xFFFFFFFF` is and `uiInputFrameCount = 2` is not — so
this would have sat correct-looking on most configurations forever.

### 2. `SetOption`'s EXT arm was missing its announcement line

`welsEncoderExt.cpp:845` logs
`"...ENCODER_OPTION_SVC_ENCODE_PARAM_EXT, LogStatisticsBeforeNewEncoding"`
immediately before the statistics block. The port had the `LogStatistics` call and
not the line above it. Restored. It was invisible until the same commit made the
arm reachable — the omission and the instrument that finds it arrived together.

### 3. The referee was normalizing away the field it was checking

Its rule 2 was `s/(0x)+[0-9a-fA-F]+/0xPTR/g` — any address anywhere. But
`LogStatistics` prints the frame size as `<w>x<h>`, so `SpatialId = 0,80x48`
contains the substring `0x48` and was being rewritten to `80xPTR`. On both sides,
identically, so it compared clean: **a port printing `80x49` would have passed**.
The rule is now anchored to `pCtx= `, the only field that actually carries an
address in a message the port emits.

**The rule this leaves behind.** A normalization is a hole you cut in your own
instrument, and cutting it wider than the thing you meant to hide is how a
referee ends up certifying the field it was built to check. Every normalization
should be anchored to the field name it applies to, not to the shape of the value.

### Calibration

S66 both ways on the *new* coverage specifically, not just the old: a planted
`uiSkipedFrameCount` typo inside `LogStatistics` drops the tally **20 -> 17** (all
three statistics lines) and surfaces as MISSING-AND-UNOWNED *and* EXTRA; the clean
tree returns 20/35 with 0 unowned, 0 extra, 0 stale. Proving the typo is caught in
the function that had no coverage an hour earlier is the point of running the
calibration again rather than trusting the first one.

---

## F187 — six of fourteen "mechanical" conversions were documented hazards, and the documentation was in the file

**Session X2, the second follow-up.** Asked which of step 1's remaining raw sites
were mechanical, this session classified fourteen as such: five `pBsWriter`, three
`CheckMatched*` parameters, and six `WelsInitSps`/`WelsBitRateVerification` layer
parameters. **Eight survived contact with the call sites. Six did not, and both
refusals were already written down in the source.**

### What went in (9 raw sites, 2 `unsafe fn`)

* **`au_set.rs`'s five `pBsWriter: *mut BsWriter` -> `&mut BsWriter`.** Every body
  opened with `let pBs = &mut *pBsWriter;` and a null check, and every production
  call site already formed the reference — `&mut (*pOut).sBsWrite`, passed beside
  `&mut (*pOut).sBsBuffer[..]` as a separate argument. Two disjoint fields of one
  `SWelsEncoderOutput`; never an aliasing question. The raw was a vestige and the
  null check was unreachable.
* **`CheckMatchedSps` and `CheckMatchedSubsetSps` — both now safe fns**, four
  `*const` parameters gone. The two call sites that held raws spell
  `&*pSpsArray.add(id)` now.

### What did not, and why

1. **`WelsWriteSVCPrefixNal`'s `pBsWriter` (`nal_encap.rs`).** Its multi-threaded
   caller passes `std::ptr::addr_of_mut!((*pSliceBs).sBsWrite)`
   (`slice_multi_threading.rs:1371`) over fork-shared slice state, and the comment
   two lines above says the spelling is deliberate. Converting the parameter would
   force a `&mut` retag over the seam. **H2's question, not this one's.**
2. **The five `WelsInitSps` / `WelsInitSubsetSps` layer parameters.** The call site
   carries its own refusal (`paraset_strategy.rs:846`):

   > S29's named shape. `WelsInitSps` takes `*mut SSpatialLayerConfig`, so the
   > reference here only existed to retag and be cast away — **and its retag is
   > what invalidated `InitDqLayers`'s live pointer into the same layer.**

   A previous session tried exactly this conversion and it broke something. The
   `addr_of_mut!` at the call site is the fix.

### The distinction the eight share and the six do not

**Shared versus unique.** `CheckMatched*` only reads, so `&*ptr` is a
`SharedReadOnly` retag that leaves every other pointer into the array standing —
which is why it is safe where F71's `&mut` was not. `pBsWriter` writes, but to a
field nothing else aliases. The six that failed all mint a `Unique` retag over
storage something else holds a live pointer into: the fork's slice buffer, or a
spatial layer `InitDqLayers` is walking.

So "mechanical" is not a property of the *parameter* — a raw pointer to a single
object with one caller looks identical in all fourteen. It is a property of what
else is live at the call site, which the signature cannot show you and the count
in a brief's table certainly cannot.

### The honest correction

The estimate of "roughly 14, give or take a few either way" was **43% wrong in the
optimistic direction**, and every one of the six had its reason in a comment
within twenty lines of the code. The lesson is this session's own, for the third
time: F179 was a comment asserting a false negative about its own tree, F181 was a
parameter name read instead of the reference, and this was a classification made
from signatures instead of from call sites. **Read the caller before you call
anything mechanical.**

---

## F188 — D-fid-4's five duplication sites are six, and the sixth is inside the acceptance instrument

**Session H2, step 0.** F184 counted five `WELS_LOG_*` duplications and H2's brief
repeated the number with the grep that produces it:

> "F184 counted **five** duplication sites including `tests/trace_callback_test.rs:41`
> ... **enumerate all five yourself** (`grep -rn 'WELS_LOG_' src tests`, both codecs; S64)."

That grep is scoped to `src tests`. The port has a sixth copy outside it, in
`rust/tools/diffharness/rust_enc/main.rs:61`:

```rust
let mut level: u32 = 3; // WELS_LOG_INFO — codec_app_def.h:323
```

Same defect, same wrong value, same citation of the header line that says 4 — in the
Rust half of **the referee that D-fid-4 names as its acceptance**. `cxx_enc.cpp:52`
takes `WELS_LOG_INFO` from the real header, so the two drivers were asking their
encoders for *different* trace levels and the referee's level check was reading the
difference as a port defect on top of the one it was actually reporting.

**Fixing the five and not the sixth would have made the acceptance worse, not
better.** With the bit mask in place, level 3 admits ERROR (1) and WARNING (2) and
**not** INFO (4): the port driver would have gone silent on every message the referee
compares, and the run would have tripped the "the PORT logged nothing" guard rather
than passing.

The rule this leaves: **an instrument that hardcodes a constant is a copy of that
constant.** A duplication census scoped to `src` and `tests` cannot see the tools
directory, and the tools directory is where the acceptance lives.

---

## F189 — D-fid-4's stated acceptance could not be met by D-fid-4, and the remap is what exposed it

**Session H2, step 0.** The ruling's acceptance is "`log_referee.sh`'s level check
goes green", and the brief repeats it as "pre-built". The check compared the two
delivered level **vocabularies**:

```bash
cut -d'|' -f1 cxx.log  | sort -u > cxx.levels
cut -d'|' -f1 rust.log | sort -u > rust.levels
cmp -s "$OUT/cxx.levels" "$OUT/rust.levels" || rc=1
```

The reference's vocabulary on the fixed row is `{2, 4}`. **All eight of its level-2
lines are `ParamValidationExt` warnings this port does not emit at all**, and all
four of their texts are rows in the gap list owned by **J**. So the set comparison
could not go green while any owned gap remained, however correct the levels were: it
was reporting missing MESSAGES in the name of wrong LEVELS. After the remap it stayed
red at `cxx [2 4], rust [4]`, which is the levels being exactly right — and that is
how the confound surfaced. A check whose green is gated on unrelated work in another
session is a check nobody can act on.

**This is F186's lesson in the other direction.** F186 was a normalization *wider*
than the nondeterminism it meant to hide, so the referee certified a field it was
built to check. This was a comparison *wider* than the divergence it could prove, so
the referee condemned a defect that was not there. Both come from the same habit:
writing the check against the shape of the data rather than against the question.

**S69's replacement** asks the two questions the capture can answer. **1a**: every
level the port delivers is a member of the header's mask (`0|1|2|4|8|16|32`) — F184's
defect shape exactly, and it needs no coverage at all to fire. **1b**: for every
message *both* sides emit, the levels agree — a real level on the wrong message.
Coverage stays CHECK 2's subject and the gap list's, where it always belonged.

Calibrated both ways (S66), because a check nobody has seen fail is not a check:

| plant | 1a | 1b |
|---|---|---|
| `WELS_LOG_INFO` back to 3 | **fires** (iLevel=3 not in the mask) | 12/12 messages disagreed |
| one `WelsLog` call INFO -> WARNING (a legal mask value) | **silent** | **2/12** (the param block, emitted twice) |
| clean tree | silent | 12 messages, 0 disagreed — **PASS** |

---

## F190 — two documents in this tree state stale numbers about the instruments they describe

**Session H2, step 0, found while quoting them.**

1. `log_referee_known_gaps.txt`'s header read "Measured on the fixed row 2026-08-26:
   reference 19 lines, port 9." The tool measures **35 and 20**. Those were
   `d0020008`'s numbers; `c55b7581` — whose own commit message reads "**Coverage
   19 -> 35 reference messages**" — updated the script, both drivers and the log, and
   left the count in the one file whose entire contract is *"a stale row is a lie
   about coverage"*.
2. `gates.sh`'s header and its D-gate-6 block both read "CAPPED AT 15 MINUTES", two
   sessions after the user amended D-gate-6 to **1200 s**. A session reading the
   script rather than the plan would have cut coverage to meet a cap that no longer
   exists.

Neither is parsed by anything, which is exactly why neither was caught, and it is the
same class as **F178** (the ratchet counting `*mut` in comments) one level up: there,
prose was read *as* data; here, data was written *as* prose and then went stale. The
rule both want: **a number a document states about an instrument is as stale-able as
a row in that instrument's allowlist, and nothing checks it.** Where the instrument
can print the number, quote the instrument; where a document must carry it, date it
and name the commit it was measured at — as both now do.

---

## F191 — the in-fork "read routes" are four different problems wearing one label, and the largest of them cannot take a shared projection at all

**Session H2, step 3.** The brief hands the seam four numbers as if they were one
workload: "`ctx_func_list`'s 106 call sites; the layer's 42 raw parameter mentions;
and the 20 `MT` tags", plus X2's ~36. Measured at the site, they are four different
shapes and only one of them is a *context read*:

| route | measured | what it actually is | dissolved by a shared context projection? |
|---|---|---|---|
| the stride/index lookups | 12 sites, 4 accessors | pure fork-constant reads | **yes — landed here** |
| `ctx_mvd_cost_*` | 8 sites | fork-constant read **plus one writer** and a `*mut u16` that lives in `SWelsMD`/`SWelsME` | partly; blocked on a 70-site `*const` cascade in a different family |
| `ctx_func_list` | **105** call sites (the brief's 106 counts the definition) | a table **re-written mid-stream** — `SetFastCodingFunc`/`SetNormalCodingFunc` run per frame | **no**: the accessor's own doc forbids holding anything derived from it across a call that can reach it again. Shared-projection is the wrong shape; this is a re-derive-every-time contract |
| the layer's 42 `: *mut SDqLayer` params | 42, in 7 files | the **layer** family, not the context | no — a context surface cannot reach them |
| the 20 `MT` tags | 18 `slice_multi_threading.rs` + 2 `nal_encap.rs` (`:367`, `:399` — the brief's `:361`/`:393` have drifted) | the fork's own machinery and the slice bitstream seam | no — F187 §1 already ruled `nal_encap`'s reason is the *slice buffer*, not the context |

**`ctx_func_list` is the load-bearing correction.** It is the largest of the four
numbers and the one a reader would attack first, and a shared projection is not
merely insufficient there — it is *unsound in the direction the number suggests*.
The table is mutable at frame cadence; the accessor exists precisely so every reader
derives a fresh sibling. Handing out `&SWelsFuncPtrList` would let a reader hold one
across the re-write. The correct end state for its 105 sites is not a projection but
the dispatch enums Phase 4b built, finished — which is a family, not a seam.

**What that leaves for the count the user confirms: zero new seam items.** The
fork-constant read this session converts needs no `UnsafeCell` crossing and no `Sync`
impl, because shared reads do not race with shared reads. The seam-item count stays
at D-mt-3's **2**.

---

## F192 — F167's stored context copy is Undefined Behavior, not "argued sound": run at last, Miri refuses it in one line

**Session H2, step 2.** F167 argued `CWelsPreProcess::m_pEncCtx` sound on its dormant
half and never ran it; F171 is what that distinction cost on the other half. Run:

```text
error: Undefined Behavior: not granting access to tag <1118667> because that would
remove [Unique for <1118916>] which is strongly protected
  --> wels_preprocess.rs:2939  let bScene = (*self.m_pEncCtx).bCurFrameMarkedAsSceneLtr;
<1118667> was created by a Unique retag at offsets [0x0..0x17f10]   (the owner Box)
<1118916> is this argument    -->  drive(pCtx: &mut sWelsEncCtx)
```

**The mechanism is stronger than any argument that had been made about it, and it is
stronger in a way that removes the escape everyone reached for.** The reasoning
available before running it — mine included, written down before the run — was that a
raw read below a live `Unique` *disables that `Unique` over the bytes read*, so the
shape is sound as long as the caller does not go on to touch those same bytes; a
property of the four call sites. That is wrong. **A reference function argument is
strongly protected for the duration of the call**: while `drive` is on the stack, no
access through any other tag may remove its `&mut`, so the *read* through the stored
copy is UB immediately, whatever the caller does afterwards, and disjointness buys
nothing. The context `&mut` covers `[0x0..0x17f10]` and contains every byte the stored
copy touches.

**The real path cannot be driven, and this is measured, not assumed.** All four
readers — `GetBestRefPicScreen` (`:1567`), `DetectSceneChangeScreen` (`:2317`),
`GetAvailableRefListLosslessScreenRefSelection` (`:2596`), `GetRefFrameInfo` (`:2899`)
— are on the screen-content path, and `RequestMemorySvc` returns
`ENC_RETURN_UNSUPPORTED_PARA` the moment `iUsageType == SCREEN_CONTENT_REAL_TIME`
(`encoder_ext.rs:1222`), so no configuration finishes `WelsInitEncoderExt`.
`GetRefFrameInfo` additionally dereferences `pVaa` as an `SVAAFrameInfoExt`, which
this port never allocates (F177). So the brief's escape hatch is the only option and
what it mints is stated in the test: the owner `Box`, the root raw as
`std::ptr::addr_of_mut!` (which is `wels_encoder_ext.rs:715` character for character),
the field copy set the way `CreatePreProcess` sets it (`:1030`), the object reached
**through the context** as `(*pCtx.pVpp)` — `ref_list_mgr_svc.rs:1539`'s own route —
and a driver taking `&mut sWelsEncCtx`, which is what 149 encoder bodies take. The
reads are the three the consumers actually perform, through the same accessors.

**It cannot fire today**, and that is the only reason this is a finding rather than a
stop. It becomes live the day Phase 10 enables screen content, and the live shape is
`WelsBuildRefListScreen(pCtx: &mut sWelsEncCtx)` calling
`(*pCtx.pVpp).GetRefFrameInfo(..)` at `ref_list_mgr_svc.rs:1539`.

**A correction to F167's own mechanism, which is worth writing down because the
conclusion survived it.** F167 predicted the hazard arrives when "stage A0 replaces
that `addr_of_mut!` with a `&mut` at the root". **Stage A0 never landed** — the root
is still `addr_of_mut!` at all four sites (`wels_encoder_ext.rs:715`, `:882`, `:1094`,
`:1712`). What creates the protected retag instead is the 149 bodies that took
`&mut sWelsEncCtx` in session H. The finding was right about the outcome and wrong
about the trigger.

**The remedy is not proposed, it is driven, and it is green.** The sibling test makes
the identical call with a **shared** context borrow: a `&T` protector forbids writes
through other tags and permits reads, so the stored copy's three reads are lawful
under it exactly where they are not under `&mut`. The fix for the four consumers is
therefore to take the context by shared borrow (or as a parameter) rather than read it
back out of `m_pEncCtx`. It is a dormant-path change and this session's brief puts
`SCREEN_CONTENT(dormant)` semantics out of scope, so it is named for J with the
executable proof attached rather than performed here.

**And a defect in the first probe, which is the finding's own S57.** The fixture was
first written as a helper returning the owner `Box<sWelsEncCtx>` by value. Moving a
`Box` is itself a retag, so the root raw derived inside the helper was already dead
when the test used it, and Miri reported a SharedReadOnly retag from a nonexistent
tag — a defect in the instrument wearing the costume of a finding. Both tests build
the shape inline now, and the reason is written where the helper used to be.

---

## F193 — two of the five "slice-returning" APIs can never return a slice, and the reason is in `codec_app_def.h`

**Session H2, step 4, S54 (read the callers first).** The brief lists five accessors
that "still return raw pointers under `cursor` tags" and asks for "a typed/slice-
returning API sized by its callers". Read the callers and they are not one problem:

| accessor | sites | what the callers do with it |
|---|---|---|
| `ctx_dq_idc_map` | 4 | `.add(did)` then read/write one `SDqIdc` |
| `ctx_ltr` | 5 | the root; 3 of the 5 are the sibling-derivation tests |
| `ctx_ltr_at` | 26 | **22 immediately deref** — `&mut *ctx_ltr_at(..)`, `&*ctx_ltr_at(..)`, `(*ctx_ltr_at(..)).field` |
| `ctx_frame_bs` | 8 | **store the cursor into `SLayerBSInfo::pBsBuf`** |
| `ctx_frame_bs_cur` | 21 | **the same store at 9**; 9 more pass the cursor into NAL writers, 3 are tests |

`SLayerBSInfo::pBsBuf` is `codec_app_def.h:640` — `unsigned char* pBsBuf`, a **public
C-ABI field of a struct this library hands to the application**. It cannot become a
slice, a reference, or anything else with a lifetime, in this phase or any other,
because the value crosses the boundary. So two of the five are **permanent raw
returns by the ABI**, not by port debt, and the right outcome for them is a note at
the accessor saying so — otherwise a later session pays S54's cost again to learn it.

**The precise count, corrected.** This finding first said "12 of the 21" for
`ctx_frame_bs_cur`, read off a truncated listing. Measured:
`grep -c 'pBsBuf = ctx_frame_bs_cur(' ` is **9**, plus 3 for `ctx_frame_bs`. The other
nine production uses pass the cursor into the NAL writers, and three are tests. **The
correction does not move the verdict but it does narrow it**: what is permanent is
that the accessor must yield something storable in a C `unsigned char*` field, which
those nine stores require. The nine writer-passes are an ordinary raw-parameter
question and belong to whoever converts `nal_encap`'s bitstream surface — not to this
row. Two counts in one table, and only one of them was load-bearing; the habit that
matters is measuring the one that is.

The remaining three are real work of two different sizes. `ctx_dq_idc_map`'s four
sites convert. `ctx_ltr_at`'s twenty-six are twenty-two that want `&mut SLTRState`
and **four that cannot have it**, and those four are already documented at their sites
(T9.G7, `ref_list_mgr_svc.rs:757`, `:1025`, `:1453`, `:1648`): the body holds the LTR
state across calls that re-derive their own `&mut` to the *same* `SLTRState`, and
"two Unique tags from one raw root are siblings, and the second pops the first". A
`&mut`-returning accessor borrows the context for the reference's lifetime, so the
borrow checker would refuse those four at the call — which is the crux arriving
exactly where H's precedent says it does, and it is four bodies of restructuring, not
a signature change.

**The count that matters for the brief's framing**: "five slice-returning APIs" is
2 permanent + 1 small + 1 medium-with-a-named-crux + 1 (`ctx_ltr`, which follows
`ctx_ltr_at`). One number, four different answers.

---

## F194 — the ratchet is a per-file rule, so test-only unsafe competes with library unsafe for headroom, and it decides where a probe may live

**Session H2, step 2, caught by the gate rather than by review.** The first home for
F192's two Miri probes was `wels_preprocess.rs`, beside the four consumers they are
about. `gates.sh family` came back:

```
INCREASES vs baseline:
  encoder/wels_preprocess.rs  unsafe_block: 0 -> 2
  encoder/wels_preprocess.rs  unsafe_fn:   45 -> 47
```

583/583 in both profiles, every test green, and the battery **FAIL** — correctly.
`wels_preprocess.rs` had been driven to **zero** `unsafe_block`, and two
`#[cfg(test)]` probes put it back above a line the phase had paid to reach. The
ratchet cannot tell test code from library code: both are text in a file under
`src/`.

**This is not F178 and it is not a defect.** F178 is the ratchet counting `*mut` in
*comments* — a false positive on prose. This is a **true** positive with a
consequence nobody had written down: **a probe's location is constrained by the
ratchet's per-file headroom**, so "put the test next to the thing it tests" is not
always available, and the alternative is either weakening the instrument
(regenerating a baseline to pass) or moving the test.

Moved, not regenerated. The probes live in `encoder_context.rs`, which is the
subject's own file — the aliasing under test is the *context*'s — and which sat four
`unsafe fn` and three `unsafe {` **below** its baseline after this session's own
conversions. The probe reads through `CWelsPreProcess::m_pEncCtx` exactly as before;
it takes the object as a parameter rather than `&mut self`, which changes nothing
about the shape, because the receiver is derived from the same `pVpp` raw either way.

**The rule for J**: when a session's own conversions free per-file headroom, that
headroom is the budget for its probes, and where the two do not coincide the probe
moves. A baseline regenerated to admit a test is an instrument quietly lowered, and
the whole point of the ratchet is that nobody notices that happening.

---

## F196 — `ctx_ltr_at`'s raw return is load-bearing at seventy-five sites, not the four T9.G7 documented, and only the compiler could say so

**Session H2, section D, attempted and reverted.** S54's rule says an accessor whose
result every caller dereferences should return the reference, and `ctx_ltr_at` is the
textbook case: **twenty-two of its twenty-six call sites** spell `&mut
*ctx_ltr_at(..)`, `&*ctx_ltr_at(..)` or `(*ctx_ltr_at(..)).field` on the spot. F193
sized the blocker at **four** bodies — `ref_list_mgr_svc.rs:757`, `:1025`, `:1453`,
`:1648` — each carrying T9.G7's note that it "holds the LTR state across calls that
re-derive their own `&mut` to the *same* `SLTRState`".

Converted (`-> &mut SLTRState`, `pCtx.pLtr[kiDid]`), the compiler reports **84 errors
across 75 distinct sites**, and the breakdown is the finding:

```
  49  E0499  cannot borrow `*pCtx` as mutable more than once
  28  E0503  cannot use `pCtx.…` because it was mutably borrowed
   5  E0502  mutable borrow conflicts with immutable borrow
   1  E0506  assignment to borrowed value
  ---
  83  of 84 are borrow conflicts.  ZERO are mechanical type errors.
```

**Zero mechanical errors is the whole result.** A `*mut` → `&mut` conversion normally
produces a hail of `E0614`/`E0308` from the `&mut *` and `(*x).f` spellings, and those
are `sed` work. There are none here: every single site that fails, fails because the
returned borrow *conflicts with another use of the context*. The raw return was not a
vestige and it was not laziness — it is what lets seventy-five sites reach the LTR
state and the context in the same breath.

**So T9.G7's four is an undercount by an order of magnitude**, and not through
carelessness: T9.G7 was documenting the sites where it had *observed* the hazard —
bodies that hold the state across a re-deriving call — which is a strictly smaller set
than the sites where a real borrow would conflict. **A comment can only record the
conflicts someone tripped over; the borrow checker enumerates the ones that exist.**
That gap, four against seventy-five, is the argument for doing this conversion *at
all*: the raw is hiding coexistences nobody has audited.

**Reverted, deliberately.** The session's own rule for this attempt was to back out if
the work turned from relocating bindings into rewriting the logic, and 83 borrow
conflicts across `WelsUpdateRefList`, `WelsMarkPic`, `WelsUpdateRefListScreen` and
`WelsMarkPicScreen` — two of them live camera-path reference-list management, in F3's
neighbourhood — is the second thing. Landing it half-done would have been worse than
either finishing or not starting.

**What J gets that H2 did not have**: the real number, the real error mix, and the
knowledge that this is a borrow-extent refactor of four large bodies plus seventy-one
call sites, not a signature change. Sized honestly it is its own session's step, and
it should be scoped as one rather than folded in as "22 easy and 4 hard".

---

## F195 — the F67 probe counts distinct *types*, not fields, so it cannot see a deletion whose type survives elsewhere — and D-dead-7's own predicted verdict was unachievable

**Session H2, the close.** D-dead-7's ruling text says "`pCurPath` deleted,
**ten → nine** on F67", and H2's brief repeats it as the step-0 acceptance:

> "**The F67 probe after both** ... : **ten → nine expected** — state
> expected-vs-actual (S60)".

Measured at the close, the probe reports **ten**. Measured again with `pCurPath`
restored to `SWelsSvcCodingParam` and nothing else changed, it reports **ten**.
**D-dead-7 did not move this number, and it never could have.**

The probe is `fn _s<T: Sync>() {} _s::<sWelsEncCtx>();`, and what it yields is one
`E0277` per **distinct type** that is not `Sync`, not one per field. `pCurPath` is a
`*mut c_char`, and `c_char` is `i8` on this target — a type `SComplexityAnalysisParam`
*also* contributes through `SVAAFrameInfo`. Deleting the field removed one of two
sources of the same type, so the error set is unchanged.

**Why this matters beyond one wrong prediction.** The count is the phase's headline
progress metric for the send-seam's retirement condition — X's row reads "F67's probe
**12 → 10 `!Sync` reasons**", F164 re-derived it and warned that "a stable total is
not a stable list", and D-exit-2's retirement is written against it. A metric that
counts types cannot measure work done on fields, and **the direction of the error is
the dangerous one**: it under-reports remaining work whenever two families share a
pointer type, and it reports *no progress* for real progress whenever they do. Two of
today's ten are already in that state (`*mut i8` and `*mut u32` both arrive through
`SComplexityAnalysisParam`, and `*mut i8` had a second source until this session).

**What J should do with it**: keep the probe — it is the only thing that answers "is
the context `Sync` yet" — but stop quoting its cardinality as a work measure. The
member table (owner per reason) is the useful artifact; the integer is not. If a
count is wanted, it has to come from a field census, not from the borrow checker's
deduplicated diagnostic set.

### And my own report of it was wrong twice, in one line

H2's step-0 commit (`70729e28`) says:

> "**The F67 probe, run after both step-0 rulings: expected nine, actual nine.**"

Both halves are false. *Expected* nine came from quoting the brief instead of
deriving it — exactly the failure mode the brief's own header warns about. *Actual*
nine came from reading my own command through `head -60`, which cut the tenth block:
`*mut SScreenBlockFeatureStorage` is reached through `SPicture` → `RecPicPool` and has
the longest `required because` chain of the ten, so it is the block a line limit eats
first. The agreement between a wrong expectation and a truncated measurement is what
made it look like a confirmation. **S60 asks for expected-vs-actual precisely so a
mismatch is visible; two errors that cancel defeat it, and the guard against that is
to capture the whole output and count it mechanically rather than to read a tail.**

---

## F197 — H3's step-0 reproduction: F196's breakdown is exact and its total is off by one; the 83 classify into five remedies and none of them is a split borrow

**Session H3, step 0 — probe applied, measured, reverted; nothing landed.** The flip
(`ctx_ltr_at` returns `&mut SLTRState`, body `&mut pCtx.pLtr[kiDid]`) reproduces
F196's census almost exactly. Expected, from F196: 84 errors / 75 distinct sites /
breakdown 49 E0499 + 28 E0503 + 5 E0502 + 1 E0506 / zero mechanical. Actual: **83
errors / 75 distinct sites / breakdown 49 + 28 + 5 + 1 / zero mechanical**. The
breakdown and site count match to the digit — and F196's own breakdown already summed
to 83 against its stated total of 84. **The tree did not move; F196's headline total
was arithmetically inconsistent with its own table when written**, and the table was
the correct half. 82 of the 83 are in `ref_list_mgr_svc.rs`, 1 in
`wels_preprocess.rs` (`SetRefMbType`).

**The classification, by function and remedy** — committed before any edit so the
execution can falsify it. Two structural facts drive every row: (1) `ctx_param`,
`ctx_vaa`, `ctx_ref_list`, `current_layer` all take `*mut sWelsEncCtx`, so each
inline call conflicts only through the implicit `&mut → *mut` reborrow-coercion of
`pCtx` — **hoisting the call and leaving the deref in place** removes the conflict
without moving any read, because the returned raw points into a different allocation
(F71) and stays valid across the LTR borrow. (2) Every "uses the LTR state and the
context in the same breath" site turns out to pair the LTR state with one of those
separate-allocation raws or with a `Copy` scalar — never with a second live borrow of
the context struct itself.

| function | path | errors | remedy |
|---|---|---:|---|
| `DeleteInvalidLTR` :423 | live | 1 | reorder: `pParamInternal` derived above `pLtr` |
| `HandleLTRMarkFeedback` :478 | live | 3 | reorder `pParamInternal` above; hoist `ctx_vaa` call to a raw root above `pLtr` |
| `LTRMarkProcess` :569 | live | 6 | reorder `pParamInternal` above; hoist `keSliceType` scalar; hoist `ctx_vaa` root |
| `WelsUpdateRefList` :761 | live | 13 | delete the held binding; **re-derive after** `LTRMarkProcess`/`DeleteInvalidLTR`/`HandleLTRMarkFeedback`, one fresh borrow per branch |
| `CheckCurMarkFrameNumUsed` :905 | live | 1 | reorder: `pParamInternal` above `pLtr` |
| `WelsMarkPic` :1029 | live | 9 | hoist `pParam` root + `kuiTid`; split the condition at `CheckCurMarkFrameNumUsed` (short-circuit preserved), re-derive for the writes; **narrow `WelsMarkMMCORefInfo`** |
| `FilterLTRRecoveryRequest` :1092 | live | 1 | reorder: `pParamInternal` above `pLtr` |
| `FilterLTRMarkingFeedback` :1139 | live | 2 | hoist `pParam` root above `pLtr` |
| `WelsBuildRefList` :1170 | live | 4 | drop the held binding; inline re-derive at the two LTR touches (condition read, `iLastRecoverFrameNum` write) |
| `WelsUpdateSliceHeaderSyntax` :1272 | live | 14 | hoist: one `bLTRMarkingFlag` bool read before the slice loop |
| `WelsUpdateRefListScreen` :1457 | dead | 12 | reorder derivations; inline LTR reads in the `pDecPic` block; re-derive after `DeleteNonSceneLTR`/`LTRMarkProcessScreen` |
| `WelsMarkPicScreen` :1660 | dead | 16 | hoist `pParam`/`pSps`/`pRefList` roots + `kuiTid`/`bSceneLtr` scalars + the MMCO layer args above one `pLtr` binding; **narrow `WelsMarkMMCORefInfoScreen`** |
| `SetRefMbType` (wels_preprocess.rs:2742) | live | 1 | reorder: derive `pLtr` inline in the condition, after the `ctx_param` read |

Remedy totals: **hoist** and **reorder** cover eleven functions; **re-derive after a
whole-context call** is the shape of the two big live/dead update bodies; **narrow
the callee** twice — `WelsMarkMMCORefInfo` and `WelsMarkMMCORefInfoScreen` each have
exactly one caller (S54's read-every-caller done) and read only two `ctx_param`
scalars plus read-only LTR fields, so they take `(scalars, &SLTRState, …)` and lose
the context parameter entirely. **Split borrow: zero sites need one.** The brief
carried `LoadPreviousStructure`'s split-borrow shape as the mid-weight remedy; the
classification finds no site where the LTR state must coexist with a second borrow
*into the context struct itself* — the one same-struct neighbour
(`bRefOfCurTidIsLtr`, written in `LTRMarkProcess` and `encoder_ext.rs:4109`) is
touched only after the LTR borrow's last use, which NLL accepts without help.
**Genuinely interleaved: zero sites** — the revert rule stands armed but the table
predicts it fires nowhere.

Two consequences worth recording. First, the conversion's staging is fully
incremental: `(*ctx_ltr_at(..)).f`, `&mut *ctx_ltr_at(..)` and `&*ctx_ltr_at(..)`
compile against **both** the raw and the reference signature, so each body can be
reshaped and gated against the raw accessor, with the flip landing last as a
near-mechanical commit — and each reshaped body is verified by re-applying the probe
flip locally (uncommitted) and watching that body's errors vanish from the census.
Second, the new body `&mut pCtx.pLtr[kiDid]` turns the old empty-Vec `null` return
and the release-mode out-of-bounds `root.add(kiDid)` into a bounds-checked panic:
every one of the 20 production call sites dereferences the result unconditionally
today, so `null` was never a survivable answer — the panic replaces UB, not
behaviour.

---

## F198 — the planted fault's ledger (S59): the ltr preset referees LTR marking at sixteen of sixteen, and mt's zero is preset design

**Session H3, step 3's tail — planted in the landed form, measured, reverted.** The
fault: the marked frame's long-term index off by one
(`pShort.iLongTermPicNum = pLtr.iCurLtrIdx + 1` in `LTRMarkProcess`, the line
carrying a comment naming it planted). The first fault moved the output, so no
escalation was needed:

- `sweep.sh ltr`: **16 of 16 configurations FAIL** — fourteen byte-different at
  identical sizes (the index is a few bits of slice-header MMCO syntax), and two
  collapsed to a fifth of the reference size (`gop=0 fb=3` at both resolutions:
  the wrong index compounds through the LTR feedback path until reference frames
  stop matching at all).
- `sweep.sh mt`: **0 of 120 rows fail** — the mt preset runs with long-term
  reference off, so the zero is the preset's design, not a coverage hole. The
  family battery's 583 rows include the ltr sixteen, so the family gate referees
  this code on every run this session made.

Clean-tree control after the revert: ltr 16/16 PASS. S59/F175's rule is the
point — a fault that fails nothing proves nothing, and this one failed
everything its preset can reach, with the zero recorded beside the reason it
explains itself.

---

## F199 — F93's four parse-only rows were one missing statement: the port never zeroed `iTotalNumMbRec` at fresh-picture prefetch, so `ResetActiveSPSForEachLayer` — gated on that count in both trees — could never fire after a dropped access unit

**Session J, step 0.** The reproduction first (S68): at `edcccc68`,
`ecref_rs res/Error_I_P.264 99999999 --parse-only` diverged from the checked-in
golden on exactly F93's four rows (7, 9, 13, 14; 0 frames emitted against the
reference's 1) — the finding's table was current to the digit.

**What the instrumented run showed.** The stream carries **three different SPSs**
(ids 0/1/2 — 352x288, 640x480, 352x288; the finding never said this, and it is why
the asset leans on `pActiveLayerSps` at every boundary). The port's
`iTotalNumMbRec` *accumulated across access units*: 132 left by the dropped AU1
(IDR starting at mb 264, EC disabled), then 132+400=532 after the mis-split IDR,
then 1201 forever. Both trees gate `ResetActiveSPSForEachLayer` on
`iTotalNumMbRec == 0` (`decoder_context.h:549`), so the stale count meant
`pActiveLayerSps[0]` stayed at SPS 0 after the sequence switched to SPS 1 — and
`CheckAccessUnitBoundary`'s first clause (`active != cur` ⇒ boundary) then **split
the one recoverable IDR access unit in two**: row 7 constructed `{IDR mb 0..399}`
alone (532 of 1200 ⇒ `dsFramePending`), row 9 constructed `{IDR mb 400..1199}`
with no slice at mb 0 (⇒ `dsBitstreamError`, nothing emitted), and rows 13/14
inherited the wreckage (`dsRefLost`/`dsBitstreamError` against the reference's
two `dsFramePending`s).

**The root is one statement of `DecodeCurrentAccessUnit`.** The reference zeroes
the count whenever it prefetches a fresh picture (`decoder_core.cpp:2568-2569`,
inside the `pDec == NULL` arm, *before* the null check):

```c
if (pCtx->iTotalNumMbRec != 0)
  pCtx->iTotalNumMbRec = 0;
```

The port's prefetch arm had the prefetch, the null check and the `bNewSeqBegin`
stamp — and not the zeroing. The same commit also restores the arm's `else if
(iTotalNumMbRec == 0)` re-stamp of `pDec->bNewSeqBegin`
(`decoder_core.cpp:2588-2590`), which the port was equally missing.

**Why seven phases of referees never saw it.** With error concealment on (every
conformance asset, all 2707 malformed rows), an incomplete frame is *concealed*:
`iTotalNumMbRec` is forced to the full count and `DecodeFrameConstruction` zeroes
it on output — the leak needs a *dropped* AU, i.e. EC disabled. On the ordinary
path with `--ec=0` (F96's referee), the leaked count changed no observable row on
this asset — `ecref` vs `ecref_rs` re-measured IDENTICAL there both before and
after this fix, in all three EC modes — because the ordinary path's divergent
observables (planes, `iBufferStatus`) never materialize for the dropped frames.
Parse-only is the one mode where the boundary mis-split becomes output: the split
decides which NALs an emitting call hands back. So F93's "parse-only is the
messenger" was right twice over — the defect was in the shared path, and only
parse-only could see it.

**After the fix**: all 18 rows of `Error_I_P` match the golden including the
emitted frame's SHA-1; the asset moves from the retired `DIVERGING` list into
`ASSETS` (the golden now referees it on every `cargo test`); the malformed 16 and
the parity suites pass unchanged; `gates.sh commit` green (the fix is
decoder-side — encoder bytes cannot move, and did not).

---

## F200 — the census tool's exact tag match silently dropped 16 annotated tags; the header everyone quoted was undercounting

**Session J, step 1.** `phase9_census.py` matched `m.group(1) not in PHASE9_TAGS`
— an *exact* string comparison — while sessions E3, G and H had annotated tags in
place (`// unsafe-cat: port-raw(Phase 9) — the in-fork *mut SDqLayer (S63, G's)`).
Fifteen annotated `port-raw(Phase 9)` tags and one annotated `cursor` tag were
therefore invisible to every census since the first annotation landed: the
committed header said **540** (and the live tool **535** — H3 moved tags and the
doc was never regenerated, despite H3's close note claiming "censuses regenerated
unchanged"), while the tree held **551** = 519 + 32. Three separate numbers for
one quantity, and none of them measured. Fixed by matching the family prefix
(`tag.split(" — ")[0]`), the same commit that regenerates the census under
D-exit-3's categories. The J brief's "507 + 33 = 540" is corrected wherever the
close quotes it.

---

## F201 — the disposition census: 551 queue-family tags classify into 204 fork-shared, 55 instrument, 20 dormant/single/ABI/dead — and a 293-item queue the phase never owned

**Session J, step 1 — the numbers behind D-exit-3.** Every `port-raw(Phase 9)`,
`cursor` and `MT` tag classified by the fork-reachability walker
(`phase9_forksplit.py`: 3 scope blocks, 5 seeds, 495 reachable bodies; the 111
ctx-param bodies split 109 in-fork / 2 ST — F166's `ParasetStrategy` and the
dormant `WelsMdInterFinePartitionVaaOnScreen`), an unsafe-call-graph join, and a
by-hand audit of every residual:

- **204 → `fork-shared(S63)`**: the in-fork closure (183 reach-listed items, the
  20 `MT` tags naming the same seam, `WelsWriteSVCPrefixNal` per F187.1).
- **55 → `instrument(test)`**: tags sitting on `#[test]` items.
- **14 → `SCREEN_CONTENT(dormant)`**: feature-search kernels installed only under
  `bScreenContent` (`svc_motion_estimate.rs:541`), the lossless-ref-selection
  preprocess surface, S57's dark trio.
- **3 → `lawful-single`** (F166; S29/F187 twice), **2 → `C-ABI`** (F193's
  `ctx_frame_bs` pair).
- **293 stay** (`port-raw` 290 + `cursor` 3): 180 ST callers of fork-kept
  accessors (unlock table: `ctx_rc_at` 38, `ctx_param` 32, `ctx_ref_list` 25,
  `current_layer` 13, …; H3's safe-sibling recipe is the proven remedy), 60
  never-owned ST-surface items (the charter's `other`, which §6.1 flagged as
  unowned and no session took), 37 safe-shaped freebies (no raw in signature or
  body), 15 slot-typed kernels awaiting exit condition 3's per-slot READ census,
  and **1 dead** (`WelsInitSliceEncodingFuncs` — empty body, zero callers in both
  trees; upstream declares it in `svc_encode_slice.h:167` and never defines it).

The queue, per item, is `phase9_disposition.md` §5. The brief's "expect few"
for the convertible remnant was wrong by two orders of magnitude, and the
honest reason is in the charter itself: the `other` family "needs roughly 2
sessions of its own — somebody has to be given them", and nobody was.

---

## F202 — the two instrument fixes, calibrated both ways; the shim and transmute metrics close at zero; the missing negative calibrations recorded

**Session J, step 2.**

**F170 (gates.sh times with wall clock).** The sweeps and the Miri lane now
measure children CPU (user+sys, the shell's `times` builtin — which must run in
the invoking shell: a pipeline or `$(...)` is a subshell whose counters read
zero, measured on this machine's bash 3.2, so the snapshots go through
`times > file`). `s61_report` compares CPU against CPU whenever the baseline
line carries `cpu=<s>`, falling back to wall for the seeding run; the regime
change is recorded in `miri_wall_baseline.txt`'s comment block beside D-gate-6's
and F168's. Calibrated both ways by driving the extracted function against
crafted baselines: a 2.3x CPU regression **warns**; a run whose wall doubled
while CPU stayed flat — the load phantom the fix exists for — **stays silent**;
the wall-only fallback regime warns and passes on its own numbers. The exit
battery also gains `FORK_PAIR=external`: F168's rule (run the fork pair
parallel, never compare serial to parallel) against the two serial fork steps
the exit level has carried since before the rule; the SKIP is loud and the
close owes the pair's own verdict beside the battery's.

**F178 (the ratchet counts comment text).** `count_all` strips `//` to
end-of-line before counting. Calibrated both ways: a planted comment carrying
`*mut u8`, `no_mangle` and `SHIM(` moves nothing (`check` rc 0 — the defect
that failed two sessions' gates on documentation is closed), and a planted
`let _p: *mut u8` in code still fails the per-file ratchet
(`safe/prng.rs raw_ptr: 0 -> 1`). Rebaselined; the totals moved by **exactly**
the pre-measured prose counts, per metric:

| metric | was | now | prose removed |
|---|---:|---:|---:|
| raw_ptr | 1338 | **1181** | 157 |
| shim | 32 | **0** | 32 |
| transmute | 4 | **0** | 4 |
| mem_zeroed | 16 | **3** | 13 |
| no_mangle | 10 | **7** | 3 |
| unsafe_block | 267 | **266** | 1 |
| unsafe_impl | 3 | **2** | 1 |
| unsafe_fn | 593 | 593 | 0 (line-anchored) |

Two metrics close at zero: every remaining `SHIM(` and `transmute` in the
library was a comment recalling deleted code. The brief's "one session measured
exactly 4 prose counts in raw_ptr" was an old tree's number — the annotated
tags G/H/E3 added and eight phases of documentation grew it to 157, and the
validation's *form* (the fix moves each total by exactly the prose count) held
to the digit.

**The F159-coda calibrations.** The fork-reachability walker's negative:
`phase9_forksplit.py --why ParasetStrategy` answers **NOT fork-reachable**
(the ST column is a live negative verdict, F166's body); its positive stands
(`--why WelsRcMbInitGom` prints the full path to a `thread::scope` seed through
the dispatch arm), and `--no-slots` drops reachability 495 → 104 bodies — the
slot arm carries 391 bodies, which is F157's blindness made visible as a
number. The hazard detector's negative calibration is **moot**: F163 retired
"q1c to zero" as a gate (its remedy for shape B produces shape A), and q1c has
been an audit instrument, not a referee, since session G.

---

## F203 — the crate-root `deny` found the two files that never had a per-file one: 13 untagged decoder shims and the trace callback. The queue grows 293 → 306 because a hole closed, not because work regressed

**Session J, step 3.** The charter makes `#![deny(unsafe_code)]` at the root an
exit condition, and the reason is exactly what happened when it landed:
`src/lib.rs` had no `deny`, so a file that also had none was **invisible to the
whole discipline**. Two were:

- **`common/deblocking_common.rs`, 13 items** — the *decoder's* deblocking
  dispatch-slot shims (`DeblockLumaLt4V_c` and siblings, `extern "C"`, raw
  `*mut u8` + `*mut i8` behind `DeblockingInit`'s table). Session C2 named this
  file as "the decoder's table" and left it there; nobody noticed that it
  carried no `deny`, so its 13 raw shims were never tagged and never counted.
  (C2's split report says **12**; the tree has **13** — one more than the
  number two sessions have quoted.) Tagged `port-raw(Phase 9)` with the owner
  named at each site, plus one `instrument(test)` on the file's own test module.
- **`common/wels_trace.rs`, 2 sites in `WelsLog`** — the application's
  `SLogContext*` deref and the call through its `pfLog`. Tagged `C-ABI`: both
  cross `codec_api.h` and neither can carry a lifetime.

**So the queue grows from 293 to 306, and the growth is a measurement, not a
regression** — D-exit-1 forbids *new* raw signatures, and these are eight
phases old. This is F200's shape a second time in one session: an instrument
whose scope was assumed rather than checked. The ratchet counted these files
all along (they are `.rs` under `src/`), which is why no metric moved; the
*tag* discipline did not see them, and the tag discipline is what the exit
condition is about.

**The rest of step 3, measured:**

- **`libc` leaves `[dependencies]`.** It had **zero uses in `src/`** — the one
  grep hit is a comment naming `libc::fprintf` as what the port does *not*
  call. Its real users are the two benches, which `dlopen` the C++ dylib
  (8 sites), so it moves to `[dev-dependencies]`: **the shipped cdylib now has
  no dependencies at all**, which is what the exit condition wanted, and the
  comparison harness keeps what it needs.
- **The root's four non-naming allows, measured on a clean build**:
  `unused_unsafe` **0** and `dead_code` **0** — both suppressed nothing and are
  deleted (the one item `dead_code` was hiding, `encoder_ext`'s `tag!` macro,
  fires as `unused_macros`; it minted allocation labels for the `CMemoryAlign`
  arena Phases 3-6 retired, and is deleted). `unused_imports` **89** and
  `unused_variables` **8** stay, each with its reason at the root — **and the
  89 is confounded**: `cargo fix` on the lint's own suggestions broke the build
  **twice**, once with `--all-targets`, because the `#[cfg(test)]` modules use
  16+ of the names the lib-only view calls unused. A lint count measured
  against one target is not a work list.
- **`cargo clippy --all-targets` completes for the first time.** It could not
  before: **45 × `not_unsafe_ptr_arg_deref`** (deny-by-default) and 1 ×
  `absurd_extreme_comparisons` are hard errors, so no session in nine phases
  had seen a clippy run finish. 44 of the 45 are in one file —
  `wels_encoder_ext.rs`, every one a method of the C-ABI boundary object Phase
  8 built, taking the application's own pointers through `codec_api.h`'s
  vtable. Satisfying the lint by marking them `unsafe fn` would move 44 items
  **onto** the ratchet's `unsafe_fn` metric, the wrong direction, and would
  change nothing a caller can do; the honest fix (validating safe wrappers at
  the boundary) is queue work. Allowed per file with that reasoning written
  down. The single `absurd_extreme_comparisons` is `manage_dec_ref.cpp:151`
  verbatim — `uiShortRefCount + uiLongRefCount <= 0` on two `uint8_t`s, where
  C's promotion makes the `<` half dead too; both trees mean `== 0` and the
  port keeps the spelling that diffs.

---

## F204 — the log referee's gap list is closed: 20/35 → 33/35 identical, the delivered level sets agree, and the two survivors are unportable by construction

**Session J, step 4.** The seven J-owned rows of
`log_referee_known_gaps.txt` are ported, each character-identical to the
reference on the fixed row:

- **`CheckProfileSetting`'s three `WELS_LOG_WARNING`s** (`encoder_ext.cpp:131`,
  `:138`, `:145`) — the port performed all three profile adjustments and
  announced none. Its `_pLogCtx` parameter was named with a leading underscore,
  which is how a dropped observable hides in plain sight; both call sites
  already had a live context to pass.
- **the CABAC→CAVLC follow-on** (`:658`), in `ParamValidationExt`'s entropy loop.
- **the frame-skip warning** (`:373`) and **both `Change QP Range` lines**
  (`:377`, `:382`) in `ParamValidation`.
- **`WelsInitEncoderExt`'s and `WelsUninitEncoderExt`'s pointer traces**
  (`:2386`, `:2250`) — including the reference's own doubled `0x`, which is
  `"0x%p"` meeting a `%p` that prints its own prefix.
- **the destructor trace** (`welsEncoderExt.cpp:136`), which goes in `Drop`
  *before* `Uninitialize` so the two lines land in the reference's order.

**Result: 20/35 → 33/35 identical, and the levels now agree** — `[2 4]` on both
sides where the port delivered only `[4]`. That is the property D-fid-4's
original acceptance was reaching for and F189 had to replace: every level-2 line
the reference emits on this row is a `ParamValidationExt` warning, so the port
could not deliver the level until it delivered the messages. It does now.

**The two survivors are permanent and say why**: `WelsInitEncoderExt() exit,
overall memory usage: N bytes` and `FreeMemorySvc(), verify memory usage (N
bytes) after free..`. Both print a `CMemoryAlign` running total, and Phases 3-6
retired the arena for owned containers — the number does not exist, and
synthesizing one would be inventing an observable. The referee's exit code
reflects the closed list: it fails on any unowned difference **and** on any row
that has stopped differing, which is how the seven were caught the moment they
were fixed (the run after the port printed seven `STALE GAP ROW` lines and
failed until they were deleted).

The header's coverage count was stale a second time (it said "port 20" from H2's
close through this session's step 4), which the file's own contract calls a lie
about coverage. Corrected, with the series recorded: 1 → 9 (X2) → 20 (H2) → 33.

---

## F205 — the `!Sync` contingency table, re-derived BY FIELD as F195 asked: **7 of `sWelsEncCtx`'s 65 fields**, five owners, and only two of them the context split's

**Session J, step 5.** F195 retired the F67 probe's cardinality as a work
measure — it emits one `E0277` per distinct *type*, so a deletion whose type
survives elsewhere reads as no progress. The replacement asks the compiler one
question per **field**: a generated probe module calls
`needs_sync::<FieldType>()` once for each of the 65 fields of `sWelsEncCtx`, so
every failing field is named at its own line and the count is per field by
construction. Each blocking reason below was then verified *transitively by
hand* rather than trusted from the diagnostic — F195's own lesson.

| field | type | what blocks `Sync` | owner |
|---|---|---|---|
| `pSliceThreading` | `*mut SSliceThreading` | the field itself | **the ctx split** (MT resources) |
| `pOut` | `*mut SWelsEncoderOutput` | the field itself | **the ctx split** (the bitstream sink) |
| `pVpp` | `*mut CWelsPreProcess` | the field itself | the preprocessor (X's family) |
| `pVaa` | `Option<Box<SVAAFrameInfo>>` | `SVAAFrameInfo`'s six plane cursors (`pRefY`/`pCurY`/`pRefU`/`pCurU`/`pRefV`/`pCurV`, all `*mut u8`) | the VAA family (X's) |
| `ppDqLayerList` | `Vec<Option<Box<SDqLayer>>>` | `SDqLayer::pRefList: *mut SRefList` (beside `pCsData`/`pEncData`/`pSrcPool`) | the layer family |
| `ppRefPicListExt` | `Vec<Option<Box<SRefList>>>` | `SRefList` → `RecPicPool` → `SPicture::pScreenBlockFeatureStorage: *mut SScreenBlockFeatureStorage` | **Phase 10** (screen content) |
| `sLogCtx` | `SLogContext` | `pLogCtx: *mut c_void`, the application's opaque handle | **permanent — C-ABI** |

**What the table says that the integer could not.** F164's twelve *types*
resolve to **seven fields**, and their owners are five, not one: two are the
context split's, two are X's family (VAA and the preprocessor), one is the
layer family's, one is **Phase 10's**, and one — `sLogCtx` — is the C ABI's and
can never retire while `SetOption(TRACE_CALLBACK)` exists. So D-exit-2's
retirement condition for the `send-seam` is now checkable and its answer is
**no, and structurally no**: the seam cannot retire in this project's current
API shape, because a `*mut c_void` the application owns is a member of the
context by design. That is a sharper statement than "eight of twelve are
outside this phase" and it is the one the comment should carry.

The seam stays, per D-exit-2. Its comment names the five owners above instead of
a type count; the two that were the ctx split's are the only ones any Phase-9
successor could have moved, and they are `pSliceThreading` and `pOut` — both
still raw at this exit, both in the queue.

---

## F206 — the perf freeze lifts on a bit-identical result and an unmeasurable one; the comparison that looked like a +59% regression is an instrument boundary, and the baseline document says so itself

**Session J, step 6 (D-gate-1's freeze ends here).**

**What is measured and green.** Both benches ran at the exit — `decode_1080p_bench`
all streams bit-identical (SHA-1s unchanged), `c_vs_rust_bench` **every row
bit-identical** across 30 rows (15 configurations x 1/4 threads), on real `lavfi`
content with `FFMPEG` set. That is the correctness half of exit condition 4 and it
passes. First bench run since the phase froze performance work, and nothing moved a
byte.

**What is not measurable from this run, and the near-miss worth recording.**
Comparing the exit run's encoder ms against `perf_baseline.md`'s Phase-0 table gives
a **median +59% Rust regression** across the 20 matched rows (+20% to +103%). It is
not a regression, and two checks caught it:

1. **The C++ side moved too, by ~2x, on rows whose code has not changed in nine
   phases** (QVGA Moving Box 0.265 -> 0.133 ms, VGA Mandelbrot 2.656 -> 1.339). A
   reference that speeds up 2x without changing is an instrument telling you it is
   not the same instrument.
2. **The bench prints the reason**: `C++ SIMD : ACTIVE (WelsCPUFeatureDetect =
   0x000006)`, where the Phase-0 baseline recorded `INACTIVE (0x000000)`. `0x4` is
   `WELS_CPU_NEON`. The Phase-0 numbers compare **scalar C++ against scalar Rust**;
   today's compare **NEON C++ against scalar Rust**. They are different quantities
   with the same units.

**And the project already knew.** `perf_baseline.md` records the flip and states the
rule this session nearly broke: *"the project's cumulative figures are chains of
**Rust-vs-Rust** spans anchored at Phase 0"* — measured with `perfpair.py`,
alternating two builds of the **port** on one machine in one sitting. An absolute-ms
comparison across twenty days, a toolchain change and a reference-capability change
is not that chain and cannot join it.

**So the position is restated from the record, not re-derived** (S24's spirit: quote
the measurement that exists rather than manufacture one):

| ledger row | last measured position | tripwire | verdict |
|---|---|---|---|
| encoder cumulative | **≈ +10…+12%** (session D's close, unmoved by E) | D-perf-4 **+25% median** | **unbreached, by ~13 points** |
| decoder cumulative (CB) | **≈ +22.6…+23.8%** (Phase 5b's close) | D-perf-5 **≈+23% stop-line** | at or ≈0.2–0.8 over, dispositioned by D-perf-6 |

**This session's own span is perf-neutral by construction and no span was run.** Its
commits are tag text, comments, lint attributes, a `Cargo.toml` dependency move, seven
`WelsLog` calls on initialisation and teardown paths, one synthetic unit test, and one
statement in the decoder's access-unit prefetch. No kernel, no dispatch table, no
allocation path, no hot loop. A `perfpair` span over that would measure its own null,
which is why none was taken — and why the freeze lifting changes no number.

**The honest gap**: the exit did **not** re-derive the cumulative figure. Doing so
needs a `perfpair` chain from session E's stashed binaries forward, which is D-perf-6's
work, not a measurement the exit can substitute for.

---

## F207 — the exit battery's Miri half was stopped by direction, and what that leaves unverified is named here rather than glossed

**Session J, step 5, by the user's instruction mid-close**: *"let's skip Miri by
now, your changes didn't really make progress for this refactoring, so we can
skip it"*. The reasoning is sound and worth writing down, because it is the
first time this project has deliberately closed a phase with the aliasing
instrument unrun: **Miri's value in this project is catching UB that a
*conversion* introduced** — F114 (session D), F148.3 (E2), F171 (H) were each a
retag created by a signature change and invisible to every byte gate. This
session performed **no conversions**. Its code changes are one decoder statement
(F199), seven `WelsLog` calls on init/teardown paths, one synthetic unit test,
and lint/tag/comment text. None of them creates a borrow, moves a lifetime, or
changes an aliasing relationship, so the probes' expected verdict is the
previous close's verdict.

**What was run and is green** (the whole non-Miri battery):

| step | result |
|---|---|
| `cargo build --all-targets` | PASS |
| `cargo test` debug / release | **561 / 554** passed, 0 failed, 20 ignored |
| unsafe ratchet | PASS, no per-file increase |
| duplicate census | PASS, 56 allowlisted |
| diffharness sweeps, **both profiles** | **583/583 and 583/583** |
| `decode_1080p_bench` | all streams bit-identical |
| `c_vs_rust_bench` | **all 30 rows bit-identical** |
| ABI export list | **7/7**, exactly upstream's seven |
| external dlopen harness | **TALLY 14/14** |
| upstream `test/api` gtest | **191/199, allowlist exactly 8** |
| log referee | **PASS, 33/35** |

**What is therefore unverified at this exit, precisely:**

1. **The whole-library Miri `--lib` step** (the unscoped one; the last green
   encoder-scoped run was H3's close at `edcccc68`, 291 passed / 0 failed).
2. **The two full-drive encode probes** (`encode_loop_runs_with_cavlc`,
   `encode_loop_runs_over_size_limited`) at `MIRI_FULL=1`.
3. **The fork/join pair** — both probes were launched in parallel per F168 and
   killed mid-interpretation. Last green: H3's close, and H's close before it at
   3411/3493 s.
4. **The differential integration tests under Miri**.

The tree's aliasing evidence is therefore **H3's close plus this session's
argument that nothing aliasing-relevant changed**, not a fresh run. Anyone
resuming should run `FORK_PAIR=external bash rust/tools/gates.sh exit` and the
parallel fork pair before treating the exit battery as complete; the commands
and the F168 form are in `gates.sh`'s header and `miri_wall_baseline.txt`.

**S61 has no reading this close** — the lane never completed, so the baseline
file keeps H3's 506 s and the cpu column stays unseeded, exactly as F170's
regime note anticipated for the first run after the change.

---

## F208 — a `&self` reader on the context is a **whole-context** `SharedReadOnly` retag, so it kills sibling `Unique`s the borrow checker never referees — T9.H14's hazard again, in the other direction

**Safe-conversion S1, checkpoint A2, caught by the session close's Miri lane and
by nothing else.** The conversion's whole design rests on `&self` readers being
the cheap, always-available path — "shared reborrows coexist, so the fork may
take them". They do coexist with each other. What they do **not** coexist with is
a `Unique` derived from the same context through a *raw* root, and the site that
proved it is one the tree had already been burned at:

```text
error: Undefined Behavior: attempting a write access using <4060821> at
alloc288118[0x788], but that tag does not exist in the borrow stack
  --> wels_encoder_ext.rs:2412   pStatistics.uiAverageFrameQP = if !(*pCtx).rc()…
<4060821> was created by a Unique retag at offsets [0x770..0x7c8]
  --> wels_encoder_ext.rs:2374   &mut (*pCtx).sEncoderStatistics[iDid as usize]
<4060821> was later invalidated at offsets [0x0..0x17f10] by a SharedReadOnly retag
  --> wels_encoder_ext.rs:2412                                       (*pCtx).rc()
```

`UpdateStatistics` holds `pStatistics`, a `&mut` into one field of the context,
across a read of another field. A2 spelled that read `(*pCtx).rc()`, which
auto-refs `&(*pCtx)` — a `SharedReadOnly` retag over **`[0x0..0x17f10]`, the
whole struct** — and a sibling read retag pops an earlier sibling `Unique`. The
old spelling, `ctx_rc(pCtx)`, formed **no reference to the context at all**
(`std::ptr::read` of the `Vec` header through `addr_of!`, F71), so there was
nothing to pop.

**The reason nothing else caught it is the reason it matters.** `pCtx` is a raw
here (`Self::ctx_ptr`), so borrowck never sees either derivation: both gates were
green — 583/583 in both profiles, 561 debug and 554 release tests — and the two
Miri shards that drive a real encode were the only instruments that failed. This
is F114's lesson restated at a new shape, and it is the argument for the plan's
§4.7 in one line.

**It is T9.H14's finding with the polarity flipped, at the same three lines.**
H14 removed `ctx_ltr(&mut *pCtx)` from between the same `pStatistics` derivation
and the same write — a `Unique` retag of the whole context — and left a comment
saying so. The comment did not generalise to the shared direction, because in the
raw world there was no shared direction to generalise to. A2 created one.

**The rule, and it belongs in every later checkpoint's checklist:**

> A `&self` accessor on `sWelsEncCtx` is a retag over the entire context. It may
> not be called while any `&mut`-derived binding into that same context is live.
> Where the context is a `&mut sWelsEncCtx`, borrowck enforces this. **Where it
> is a raw pointer, nothing does but Miri**, so every such body is read by hand
> and the session close's Miri lane is the gate.

The remedy is §4.6's first, as everywhere else: the read moves above the `&mut`.
A scanner for the shape is installed as
`rust/tools/f208_reader_retag_scan.py` (a `&mut (*pCtx).field` or `&mut *pCtx`
derivation live across a reader call, in a body whose context root is raw); it
exits non-zero on any body whose reader sits at or after the retag. Run over the
tree after the fix it reports **one** candidate, this one, `ordered` — the reader
now above the retag. Re-run it after every checkpoint that adds a reader; S2's
brief makes that a step.

**A second-order note on `addr_of_mut!`, which is *not* affected**: a raw made
with `addr_of_mut!((*pCtx).f)` inherits the parent's tag rather than minting a
child, so a sibling `SharedReadOnly` does not pop it — writing through it pops
the shared child instead, which is allowed. The hazard is specifically the
`&mut`-shaped derivation. That asymmetry is why F71's spelling was chosen in the
first place and why the accessors that must hand out *writable* raws keep it
(`ctx_ref_list_raw`, A3).

---

## F209 — the brief's per-checkpoint cascade estimates are per-accessor, but the de-unsafe cascade is per-**body** and gates on the last accessor a body uses

**S1, measured at A2's close.** The brief bills A2 as "`ctx_rc_at` (60): the rate
controller's per-layer state; `rc.rs`'s unsafe-fn count should collapse in the
cascade", and the plan's §5 table puts a number on it: "finishes `rc.rs` (−55
unsafe fn)". A2 converted all 60 sites and `rc.rs` lost **two** `unsafe fn`
(ratchet, per file: 55 → 53).

The arithmetic is not wrong about `ctx_rc_at`; it is wrong about what makes a
body unsafe. `rc.rs`'s bodies call `ctx_param` (37 conflict sites alone),
`ctx_vaa`, `ctx_func_list` and `current_layer` as well, and a body sheds `unsafe`
only when its **last** unsafe callee does. So the cascade is not additive per
accessor — it is a join, and it lands at whichever checkpoint converts the
straggler. For `rc.rs` that is A7 (`ctx_param`), not A2.

**What the whole session's ratchet shows, by file** (baseline `c521b616` → close):

| file | `unsafe fn` | `raw_ptr` |
|---|---|---|
| `encoder/encoder_context.rs` | 24 → **15** | 95 → **59** |
| `encoder/rc.rs` | 55 → 53 | 21 → 17 |
| `encoder/svc_encode_slice.rs` | 81 → 79 | 117 → 117 |
| `encoder/encoder_ext.rs` | 38 → 38 | 73 → 72 |
| `encoder/paraset_strategy.rs` | 12 → 12 | 18 → 15 |

The accessor layer's own file is where the count moves, because that is where the
converted items live. Everything else is waiting on its straggler. **The
consequence for planning**: stage A's value is not visible in `unsafe_fn` until
A7 lands, and a session that measures itself on that metric mid-stage will read
its own progress as a failure. The metric that does move honestly is
`raw_ptr` (1181 → 1137 across S1) and the plan's tracking number (627 → 613).

---

## F210 — `ctx_dq_layer` cannot convert in stage A, and the blocker is not the one the brief pairs it with

**S1, checkpoint A3.** The brief pairs `ctx_dq_layer` with `ctx_ref_list` because
they are "held **jointly** in the encode loop — join analysis first,
combined-accessor likely". Measured:

1. **There is no joint-holder problem to solve on the single-threaded side.**
   `phase9_ctx_join.py` reports **0 LIVE** hazards at the session's start and 0 at
   its close; all 19 (now 14) are moot, i.e. in-fork, where S63 forbids the retag
   the analyser models. The combined accessor A3 did mint
   (`ref_list_and_ltr_mut`) was needed for a *different* pair — the reference
   list and the LTR state — which the brief does not mention.
2. **The real blocker is the layer's own ownership.** The fork **writes** the DQ
   layer: F191's fourth row counts 42 in-fork `: *mut SDqLayer` parameters, and
   plan §10.3 assigns them to D2–D3. So `dq_layer` can return neither `&mut`
   (S63 forbids N workers holding one) nor `&` (the writes are real), and
   `Option<Box<T>>` offers no *safe* way to hand out a writable raw without
   retagging the pointee — the one spelling that works, `ptr::read` of the slot,
   is the `unsafe` the conversion is trying to remove.

`ctx_dq_layer` therefore stays raw through stage A and converts when the layer's
storage does. Its 15 sites are not stage A's work, and a later session should not
re-derive this at cost: the ordering constraint is **layer-before-accessor**, not
the reverse.

---

## F211 — two raw routes survive stage A by **provenance**, not by debt, and the slice readers are weaker than the header-read spelling they replace

**S1, A3 and A4.** Two derivations resisted conversion for the same reason, and
it is worth stating once because it will recur at A5–A7:

* `WelsInitCurrentLayer` stamps `SDqLayer::pRefList`, a raw *field* (stage C's),
  and the fork reads the stored value for a whole frame. The value must therefore
  carry the reference list's **own** provenance. A `&mut`-derived cast would
  stamp a fresh `Unique` that the next `ref_list_mut` call pops, leaving the fork
  reading through a dead tag — and F208 is the proof that no byte gate sees that.
  So `ctx_ref_list_raw` survives, with one caller and a note.
* `RequestMemorySvc`'s `pSps`/`pSubsetSps`/`pPps` cursors must survive the
  parameter-set calls, which reach the same arrays again. They are derived from
  the **readers**, deliberately: `as_ptr` through a shared borrow is what makes
  the coexistence lawful.

**And a real weakening, recorded rather than glossed.** The old root accessors
read the buffer pointer out of the `Vec` *header* (`(*addr_of!(v)).as_ptr()`), so
the raw they handed out carried the **buffer allocation's own** tag and survived
every later retag of the array. `sps_array()` and its siblings return `&[T]`,
which is a `SharedReadOnly` retag **over the buffer**, so a raw derived from one
is a child and dies at the next `*_mut` reader on the same array. Nothing in the
tree does that today — the session close's Miri lane drives the whole init path
and is green, 291/291 — but the property is now a *checked* invariant rather than
a structural one. A later session that adds an array writer between a cursor's
derivation and its use will be told by Miri and by nothing else.

---

## F212 — A6 priced before it is chosen: take the flip, leave the dispatch enums to C1

**S1, the pricing plan §10.1 asks for, done from measurement rather than
carried forward as an open question.** F191 ruled that `ctx_func_list`'s 105
sites "cannot take a shared projection at all", because the table is re-written
at frame cadence and "handing out `&SWelsFuncPtrList` would let a reader hold one
across the re-write". Measured at the site, the objection is answered by the
conversion itself:

* **The re-write is two fields, in one body.** `SetFastCodingFunc` /
  `SetNormalCodingFunc` write `pfIntraFineMd` and
  `sSampleDealingFuncs.pfMdCost`, and nothing else. They have exactly one caller
  (`encoder_ext.rs:2474`/`:2476`), which already derives **one** `&mut
  SWelsFuncPtrList` from the owner (`let fl: &mut SWelsFuncPtrList = &mut
  *ctx_func_list(pCtx);`) — that is `func_list_mut(&mut self)` character for
  character.
* **The caller takes `&mut sWelsEncCtx`, so it is single-threaded**, and the fork
  never writes the table at all.
* **Under the flip, "a reader holds one across the re-write" stops being a
  hazard and becomes a compile error**: the re-writer needs `&mut`, so no `&` can
  be live across it. F191's concern is exactly what borrowck refuses. The only
  residue is the raw-rooted case F208 names, and that is a per-checkpoint hand
  audit plus the Miri lane.
* **The dispatch-conflict shape is already absent**: a grep for the pattern
  F191 worried about — a table read whose call takes the context as an argument
  — returns **zero** sites, so §4.6's copy-the-fn-pointer-first rule costs
  nothing here.

**The alternative, priced.** F191's preferred end state is "the dispatch enums
Phase 4b built, finished". That is a different debt: it retypes the fn-pointer
*aliases* and every impl behind them, and it does **not** remove `ctx_func_list`
— the accessor would still be there, still raw. The plan already schedules it, as
**C1**, "the 10 unsafe fn-pointer aliases → safe signatures … lands as one change
so the table is never half-typed".

**So: A6 takes the flip** — `func_list(&self)` for the ~104 readers,
`func_list_mut(&mut self)` for the one re-writer — and leaves the enums where the
plan already puts them. The two are orthogonal: the flip removes the accessor's
`unsafe`, C1 removes the aliases'. Doing the enums inside A6 would pull a
C-stage checkpoint forward and still leave A6's own job undone.

---

## F213 — `SVAAFrameInfoExt` had a second declaration with a different layout, and only its deadness hid it

**S2, checkpoint A5.** `ctx_vaa`'s sixteen screen-content downcasts were not
sixteen spellings of one cast. Thirteen cast to
`wels_preprocess::SVAAFrameInfoExt` — the canonical port of the C type, which
`abi_guard.rs:254` pins at 1376 bytes — and three cast to a *local*
`svc_mode_decision::SVAAFrameInfoExt_t`:

```rust
pub struct SVAAFrameInfoExt_t {        //  the canonical type:
    pub sVaaBase: SVAAFrameInfo,       //    sVaaFrameInfo: SVAAFrameInfo,
                                       //    sComplexityScreenParam: SComplexityAnalysisScreenParam,
    pub sScrollDetectInfo: SScrollDetectionParam,
    pub pVaaBestBlockStaticIdc: *mut u8,
}
```

The twin omits `sComplexityScreenParam`, so **every field it named sat at the
wrong offset**: `sScrollDetectInfo` by the size of the omitted member,
`pVaaBestBlockStaticIdc` by that plus four arrays and two `i32`s. The three
consumers (`JudgeScrollSkip`, `MdInterSCDPskipProcess`, `SetBlockStaticIdcToMd`)
would have read scroll vectors out of complexity counters.

**Nothing saw it, and the reason is a stack of fences.** `abi_guard` size-asserts
the canonical type and never mentions the twin. The tree's duplicate-type census
(`find_dup_types.sh`) is name-based, and the names differ by a suffix. And all
three consumers sit behind `SCREEN_CONTENT(dormant: Phase 10)` — the port never
installs `SetScrollingMvToMd` (`encoder_ext.rs` assigns `SetScrollingMvToMdNull`
at the only site), and `pfSCDPSkipDecision`'s judging arm needs
`iUsageType == SCREEN_CONTENT_REAL_TIME`, which no diffharness preset expresses.
A wrong layout in code that cannot run is invisible to every byte gate, the
ratchet, Miri and the census at once.

**The general point, and it is F177's with the polarity flipped.** F177 said a
permanently-null field with live readers looks exactly like a dead field. This
says a *structurally wrong* declaration with dormant readers looks exactly like a
correct one — and that the thing which finds it is not an instrument but the
conversion: naming the downcast once forced the question "cast to *which*
`SVAAFrameInfoExt`?", which no site had ever had to answer. The twin is deleted;
the canonical type is what `sWelsEncCtx::vaa_ext` hands out.

---

## F214 — A6's flip moves nothing the ratchet counts, and that is the measurement F212 asked for

**S2, checkpoint A6.** `ctx_func_list`'s 121 textual sites converted to
`func_list`/`func_list_mut` and the ratchet reads **`raw_ptr` +0, `unsafe_fn` +0,
allows 612 → 612**. Nothing regressed; nothing moved.

The reasons are worth separating, because two of them are structural and one is
an artefact:

1. **F209 again, and harder.** Every body that reads the table also calls
   `ctx_param` or `current_layer`, so none of them sheds `unsafe` here. The
   dispatch-heavy files the plan bills to A6 (`svc_base_layer_md.rs`,
   `svc_mode_decision.rs`, `svc_encode_mb.rs`) collapse at A7 or later, not here.
2. **Two derivations survive as `ctx_func_list_raw`**, so the accessor's own
   `raw_ptr` and `#[allow]` do not leave the tree — they move to a smaller,
   named route. `ParasetStrategy` and `SetOption` both hold the context as a raw
   *and* write through the table, and `func_list_mut` needs `&mut self`; taking
   it would be a whole-context `&mut` retag through a raw root, which is the
   shape S63 forbids and the session's two prohibition checks count.
3. **The value A6 actually delivers is not countable.** F191's objection — a
   reader holding a projection of the table across the frame-cadence re-write —
   is now a **compile error**: `PreprocessSliceCoding` needed nine context reads
   lifted above its `&mut`, and the compiler is what demanded them.
   `InitFunctionPointers` needed one, `WriteSavcParaset` two. Before the flip
   those coexistences were invisible; after it they are unrepresentable wherever
   the context is a reference.

**What this says about the plan's tracking number.** §9 makes
`#[allow(unsafe_code)]` outside `src/api/` the single progress figure, and a
checkpoint that converts 121 sites and moves it by zero reads as a wasted
half-day under that metric. It was not: it retired the last accessor A7 depends
on and turned a class of hazard into a class of compile error. F209 said stage
A's value is invisible in `unsafe_fn` until A7; this says the same of the
tracking number, and adds the reason F209 did not have — that a *named survivor*
keeps the allow alive even when the accessor it replaced is gone.

---

## F215 — a `&mut self` accessor mints a **fresh** `Unique` every call, so two cursors taken off two calls cannot coexist — and that, not `&self`, is what A7 tripped on

**S2, checkpoint A7, and the session's mid-checkpoint Miri run is the only thing
that saw it.** F208 taught the session to watch `&self` readers: a shared retag
over the whole context pops sibling `Unique`s. A7 hit the *other* half of the
same mechanism, one allocation further in:

```text
error: Undefined Behavior: trying to retag from <640129> for SharedReadOnly
       permission at alloc289078[0x0], but that tag does not exist in the
       borrow stack for this location
  --> encoder_ext.rs:1269   if (*pParam).iUsageType == SCREEN_CONTENT_REAL_TIME
<640129> was created by a Unique retag at offsets [0x0..0x4d0]
  --> encoder_ext.rs:1072   let pParam = (**ppCtx).param_mut();
<640129> was later invalidated at offsets [0x0..0x4d0] by a Unique retag
  --> encoder_context.rs:1267   self.pSvcParam.as_deref_mut()
```

`RequestMemorySvc` bound the parameter block once and read it two hundred lines
later; in between it called `AcquireLayersNals`, which called `param_mut()`
again. **Each `param_mut()` is a fresh `Unique` retag over the whole 0x4d0-byte
block**, so the second call popped the first call's tag. A second run found the
same shape at `encoder_ext.rs:808`, where `InitDqLayers` derives *two* cursors
from *two* `param_mut()` calls one line apart — the second pops the first.

**The part that matters for the remaining stage-A work.** F71's `addr_of_mut!`
asymmetry — "a raw made with `addr_of_mut!` inherits the parent's tag rather than
minting a child" — was the session's standing answer to cursor invalidation, and
S29 leans on it by name at three of these sites. It still holds, and it is no
longer sufficient: it protects the cursor from *sibling* retags, and what changed
is the **parent**. Under `ctx_param` the parent was a slot read, identical on
every call; under `param_mut` there is a new parent per call, and every earlier
cursor hangs off a dead one.

So the rule the accessor layer needs is one line longer than F208's:

> A `&mut self` accessor is a fresh whole-struct `Unique` **per call**. Two raw
> cursors into that struct may only coexist if they come off the **same** call.
> A cursor that must outlive a later call keeps the slot-read root (F71), which
> is what `ctx_param_raw` and `ctx_ref_list_raw` exist for.

A7 acted on it: the 230 sites that merely read or write a field go through
`param`/`param_mut`, and the **26 per-layer cursors** — the ones held across a
call that reaches the parameters again — come off `ctx_param_raw`, F211's
provenance category with a third member.

**And the instrument note.** The `f208_reader_retag_scan.py` shape does not catch
this: its `uq` pattern is a `&mut`-shaped derivation *written in the body*, and
here the offending `Unique` is minted inside the accessor. The byte gates were
green — 583/583 in both profiles, 561 debug and 554 release tests — through both
occurrences. Running Miri **inside** the checkpoint rather than at the session
close is what turned two silent UB sites into two twenty-minute fixes, and S2's
brief asked for exactly that "for A7 especially".

---

## F216 — A7's cascade does not land, and the straggler is `current_layer`, not `ctx_param`

**S2, checkpoint A7, measured at its close.** F209 ruled that a body sheds
`unsafe` only when its **last** unsafe callee does, and named the join: "`rc.rs`,
`ref_list_mgr_svc.rs`, `encoder_ext.rs` shed `unsafe fn` only when their last
unsafe callee does, and for most of them that is this one" — `ctx_param`. S2's
brief repeats it: "Expect the F209 cascade to land here… Expect A7 to move [the
tracking number] far more than A5 and A6 together."

Measured at A7's close, with `ctx_param` gone from all 258 of its sites:

| metric | A6 close | A7 close |
|---|---:|---:|
| `raw_ptr` | 1117 | **1095** |
| `unsafe_fn` | 579 | **579** |
| allows outside `src/api/` | 612 | **612** |

Zero. The reason is one grep:

```
$ grep -rn 'pub unsafe fn' src/encoder/{svc_encode_slice,encoder_context}.rs \
      | grep -E 'current_layer|ctx_dq_layer|layer_|ctx_ref_pic|ctx_pic_ref'
current_layer  layer_sps  layer_subset_sps  layer_pps  layer_ref_pic
layer_enc_pic  layer_rec_view  layer_ref_feature_storage  ctx_dq_layer
ctx_ref_pic  ctx_pic_ref                     ← every accessor still `unsafe fn`

current_layer 158   layer_ref_pic 45   layer_pps 30   ctx_dq_layer 22
```

**Every unsafe accessor left in the layer is the DQ-layer family**, and nothing
else is: `ctx_ltr_at`, `ctx_sps` and `ctx_pps` became safe fns at T9.H3/A4, and
A1–A7 took the other eight. `current_layer` is `ctx_dq_layer` wearing a different
name — it resolves `iCurDqLayer` through `ppDqLayerList` and returns the same
`*mut SDqLayer` — and **F210 deferred that whole family to D2–D3**, because the
fork *writes* the DQ layer, so the accessor can return neither `&mut` nor `&`
until the layer's storage moves. At 158 sites it is in essentially every body
that also used `ctx_param`.

So the join F209 identified is real, and its straggler was never `ctx_param`:
stage A converts eight of the god-struct's accessors, and the ninth — deferred by
an ordering constraint stage A cannot lift — holds the entire cascade.

**What this costs the plan.** §9 makes `#[allow(unsafe_code)]` outside `src/api/`
the single progress figure, and stage A closes having moved it **627 → 612** — a
fifteen-line change for the ~640-site inventory §3 bills as the enabler. That is
not stage A failing; it is the metric being unable to see structural work
(F214 says the same of A6 alone). But it does mean the plan's own sequencing
claim — "the enabler: ~200 of the remaining unsafe fns are unsafe *only* because
they call these" — is **false as stated**: ~200 of them are unsafe because they
call these *and* `current_layer`. The two must land together to be visible, and
only D2–D3 can land the second.

**The consequence for S3.** D2's brief should open with the layer, not the slice
core: `current_layer`/`ctx_dq_layer` is where stage A's deferred value is stored,
and every checkpoint that converts it collects a cascade three stages deep.

---

## F217 — B1 is an MT-seam checkpoint, not a mechanical one: all four ctx singletons are reached **in-fork**, and one of them hands out a `&mut` there

**S2, measured at the stage-A close, before opening B1.** The plan bills B1 in one
line — "`pOut`/`pVpp`/`pSliceThreading` → `Option<Box<T>>`; `mutexSliceNumUpdate`
→ owned `Mutex<()>` (deleting `WelsMutexInit/Destroy` and the `Box::into_raw`
dance)" — which reads as bookkeeping. Cross the four field names against
`phase9_forksplit.py --list`'s in-fork body set and it is not:

| field | sites | in-fork bodies that touch it |
|---|---:|---|
| `pOut` | 125 | `slice_bs_buffer`, `slice_writer` |
| `pVpp` | 46 | `ctx_pic_ref`, `WelsSliceHeaderExtInit` |
| `pSliceThreading` | 28 | `thread_bs_buffer`, `DynSlcJudgeSliceBoundaryStepBack` |
| `mutexSliceNumUpdate` | 8 | `DynSlcJudgeSliceBoundaryStepBack` — the fork's own lock |

**And `pOut`'s in-fork use is a `&mut`, not a read:**

```rust
pub unsafe fn slice_bs_buffer<'a>(pEncCtx: *mut sWelsEncCtx, …) -> &'a mut [u8] {
    if (*pSliceBs).pBs.is_some() { thread_bs_buffer(pEncCtx, …) }
    else { &mut (&mut *(*pEncCtx).pOut).sBsBuffer[..] }        // ← in-fork `&mut`
}
```

That is the shape S63 forbids and D1's two audited `unsafe impl`s exist to
license for the *reconstruction picture*. Under the accessor design an in-fork
body may only take the `&self` reader — so an `Option<Box<SWelsEncoderOutput>>`
cannot simply replace the raw field: the bitstream buffer needs the same
treatment `RecPicView`/`SharedCells` gave the picture, **or** a demonstration
that the `else` arm is unreachable under MT.

**The fence, measured, so B1 starts from a condition rather than a hunch.**
`sSliceBs.pBs` is `Some` exactly when `InitSliceBsBuffer` was called with
`bIndependenceBsBuffer`, and that is computed at one site
(`svc_encode_slice.rs`, `ReallocateSliceList`'s prologue and its siblings):

```rust
let bIndependenceBsBuffer = (*pCtx).param().iMultipleThreadIdc > 1
    && kuiSliceMode != SliceMode::SM_SINGLE_SLICE;
```

So the `pOut` arm is taken when the encoder is single-threaded **or** the layer
is single-slice. The first disjunct is not the fork. The second is the open
question: `iMultipleThreadIdc > 1` with `SM_SINGLE_SLICE` still selects the
threaded path in `slice_multi_threading.rs`, and whether it spawns a worker that
reaches `slice_bs_buffer` is a *probe*, not an argument — one `mt` sweep preset
with a single-slice configuration, or a counter in the `else` arm. **B1 runs that
probe before it designs anything**, because the answer decides between "delete
the arm's in-fork reachability with a fence" and "give `pOut`'s buffer D1's
treatment".

**Why this is worth a finding rather than a note.** Stage A's seven checkpoints
were all one shape: a raw accessor becomes a reference pair, and the compiler
enumerates the work. B1 is the first checkpoint where the *storage* moves, and
the plan's §5 table gives it the same one-line weight as `ctx_rc_at`. S2 stopped
at the stage-A boundary rather than opening it half-scoped (D3's
drop-from-the-end), and S3 should price it as an MT-seam checkpoint: fork/join and
mid-row Miri probes gating every landing (§4.7), and F215's rule — a `&mut self`
accessor is a fresh whole-struct `Unique` **per call** — applying to every
per-slice reach into a singleton.

---

## F218 — F191's "42 in-fork `*mut SDqLayer` parameters" is 42 tree-wide and **11** in-fork, and the correction cuts D2's headline in four

**S2, measured while writing S3's brief rather than re-quoting.** F191's fourth
row, carried forward verbatim by F210 and by the plan's §10.3 ("The layer's 42
in-fork `*mut SDqLayer` parameters … are named to D2–D3's handle redesign"), is
two different counts run together:

```
$ grep -rn ': \*mut SDqLayer' --include='*.rs' src/ | wc -l
42                                    ← parameter declarations, tree-wide

$ # …crossed against phase9_forksplit.py --list's in-fork body set:
in-fork: 11 `*mut SDqLayer` parameters across 11 bodies   (of 98 in-fork bodies)
```

42 is every such parameter anywhere — init, teardown, the single-threaded frame
loop and the fork together. **Eleven** are in bodies the fork can reach, which is
the number that decides whether `dq_layer` can hand out a reference.

**What it does and does not change.** It does *not* change F210's ruling: eleven
fork-reachable bodies taking a writable layer pointer is still eleven too many
for a `&mut` return (S63) and still means the layer's storage moves before its
accessor does. It does change the **size** the plan attaches to that work: D2's
handle redesign has to satisfy eleven in-fork signatures, not forty-two, and the
other thirty-one convert under the ordinary single-threaded rules stage A has
been using all along. A session that prices D2 off the 42 will over-plan it by a
factor of four; one that prices it off the 11 and forgets the other 31 will
under-plan the cascade. Both numbers, both labelled, is the fix.

**And the general point, which is this session's third instance.** F190 recorded
two documents stating stale numbers about the instruments they describe; S1's
prohibition-2 figure of 22 is quoted in four commit messages with no command
attached and no spelling reproduces it; this is a number carried through three
documents with its qualifier attached to the wrong noun. The rule the plan
already has — "every count you quote carries the command that produced it" —
is the one that would have caught all three, and it is cheap: the two greps above
are eight seconds.

---

## F219 — the tracking number at S1's close is **613**, not 614, and the commit that "corrected" it moved it the wrong way

**S2, measured at its own close, because the plan's ground rule says to.** The
session log, S1's hand-off brief and the plan's §9 all carry **614** for the
`#[allow(unsafe_code)]` count outside `src/api/` at S1's close, and S1's last
commit is titled *"docs(safeplan): the close's tracking number is 614, not 613"*.
Counted at that tree:

```
$ git archive 5cc4bc4b rust/crates/openh264-rs/src | tar -x -C /tmp/s1snap
$ cd /tmp/s1snap/rust/crates/openh264-rs
$ grep -rn '#\[allow(unsafe_code)\]' --include='*.rs' src/ | grep -v '^src/api/' | wc -l
613
$ grep -ro '#\[allow(unsafe_code)\]' --include='*.rs' src/ | grep -v '^src/api/' | wc -l
613          # occurrences, not lines — the two agree, so it is not a two-per-line artefact
$ grep -rn '#\[allow(unsafe_code)\]' --include='*.rs' src/ | wc -l
658          # api included; 45 in api, which is what §3 records
```

613. The correcting commit moved a right number to a wrong one.

**Why it is worth a finding and not a footnote.** The figure is the plan's
**single progress number** (§9), quoted in every session close and used to judge
whether a stage earned its keep. S2 opened by quoting 614 from the brief and
would have reported "614 → 612, −2" — twice the movement that actually happened.
The real S2 delta is **613 → 612, −1**, and stage A's whole delta is **627 →
612**.

This is the third counting defect S2 found in one session: F218 (a qualifier
attached to the wrong noun, carried through three documents), S1's prohibition-2
figure of 22 quoted in four commit messages with no command attached and no
spelling that reproduces it, and this. All three were caught the same way, by the
rule the plan already has — *every count you quote carries the command that
produced it* — and all three would have been caught earlier if the rule had been
applied to the number being *corrected*, not just to the number being reported.
The cheap fix for the tracking number specifically: it has one canonical command,
so put it in a tool beside the other three rather than in prose.

---

## F220 — S2 closes with the two MT probes **unrun**, by direction, and this is what that leaves unverified

**S2, at the close.** Plan §4.7 makes the fork/join and mid-row Miri probes
load-bearing "for anything touching the MT seam … in every `session`-level gate
that touches `svc_encode_slice.rs`, `slice_multi_threading.rs`, or
`rec_view.rs`". A7 touched the first two. `gates.sh session` does **not** run
them — its lane filters `--skip 'fork_join_encodes'` by construction — so they
were started by hand, twice, and neither run reached a verdict: this repo sets
`[profile.dev] opt-level = 3`, every `cargo miri test` invocation recompiles the
crate at that level, and both attempts were killed in the compile (21 min and
~12 min). The session closed on the user's direction to proceed without them.
Recorded here rather than glossed, which is F207's own precedent.

**What *is* verified at the close**, and it is most of the seam:

* `gates.sh session` PASS — Miri 291/291 across four shards, including
  `encode_loop_runs_with_cavlc` and `encode_loop_runs_over_size_limited`, both of
  which drive a real encode through `svc_encode_slice.rs`.
* Both diffharness sweeps 583/583 byte-identical, in both profiles. Those presets
  include `mt`, so the multi-threaded path is exercised for **output equality** —
  what they cannot see is a retag that does not change a byte, which is the whole
  reason the probes exist (F114, F208, F215 are three such).
* Prohibition 1 (101 `*mut sWelsEncCtx` bodies against 15 writers, no
  violations) is the static half of S63, and it is green.

**What is not verified**, stated exactly:

1. **No fork/join data-race check ran against A5–A7's tree.** The two audited
   `unsafe impl`s (D1) have that pair of probes as their entire proof obligation.
   Nothing in A5–A7 touched `rec_view.rs` or the `SharedCells` scheme, and no
   checkpoint added an in-fork `&mut` — prohibition 1 says so — so the *expected*
   result is green; that is an argument, not a run.
2. **The mid-row boundary case specifically.** A7 changed
   `ReallocateSliceInThread` and `ReallocSliceBuffer`, both on the dynamic-slicing
   path the mid-row probe exists to drive, replacing a `*mut SSliceArgument`
   cursor with a copied-out `Copy` field. Byte-identical across 583 sweeps in two
   profiles, and the size-limited encode loop passed under Miri — but the
   *threaded* mid-row case is the one that did not run.

**S3 runs both probes before its first conversion**, against this tree, so that a
failure is attributed to S2 rather than to S3's own first checkpoint. Budget
~25 minutes each, most of it compile; the cheap version is to run them once,
serially, immediately after checking out.

---

## F221 — F218's "11 in-fork `*mut SDqLayer` parameters" is the count of bodies taking **both** a ctx and a layer pointer; the fork-reachable figure is **38 of 40**, and S3's ordering decision turned on it

**S3, measured before the first conversion, because the brief's own ordering
recommendation rests on this number.** F218 corrected F191's "42 in-fork
`*mut SDqLayer` parameters" to "42 tree-wide, **11** in-fork", and drew the
consequence: "D2's handle redesign has to satisfy eleven in-fork signatures, not
forty-two, and the other thirty-one convert under the ordinary single-threaded
rules stage A has been using all along." S3's brief carries it forward as the
sizing fact behind *"the eleven are what a handle redesign must satisfy"*.

`phase9_forksplit.py` has a `--type` option, and asked about the layer directly it
answers something else:

```
$ python3 ../../tools/phase9_forksplit.py --type SDqLayer --list | tail -3
**total**                              38             2
40 bodies carry a `*mut SDqLayer` parameter.
```

**38 in-fork, 2 ST-flippable.** F218's method is recoverable from its own text —
it crossed the layer parameters against "`phase9_forksplit.py --list`'s in-fork
body set", and that set is the **98 bodies carrying a `*mut sWelsEncCtx`**, a
different population. Reproduced:

```
$ comm -12 <ctx in-fork bodies> <SDqLayer in-fork bodies> | wc -l
12          # bodies taking BOTH parameters; F218 records 11
```

So the "11" is real, and it counts something — bodies that take a context pointer
*and* a layer pointer — but not what its sentence claims. A body taking
`*mut SDqLayer` need not take `*mut sWelsEncCtx`; 26 of the 38 do not.

**This is the third instance of the same defect, and F218 is the finding that
names it.** F218's closing paragraph diagnoses F191 as "a number carried through
three documents with its qualifier attached to the wrong noun", and prescribes
the rule that would have caught it — *every count you quote carries the command
that produced it.* F218 then quotes 11 with a `wc -l` for the 42 and a prose
gloss for the 11, and the gloss is the wrong noun. The tool had a `--type` flag
the whole time; the correct measurement is one command and eight seconds.

**What it changes.** Not F210's ruling — the fork writes the layer either way. It
changes the **price and the risk class** of D2, and therefore S3's opening move:

* F218 sizes the redesign at 11 in-fork signatures with "the other thirty-one
  convert under the ordinary single-threaded rules". The real split is **38
  in-fork, 2 ST**. There is no thirty-one; there is a two.
* S2's brief recommends S3 open with the layer rather than with stage B. That
  recommendation was made against the 11.

**S3's decision, recorded as the brief asks.** Stage B first, and within it
B3 → B2 → B1 rather than the plan's B1 → B2 → B3. The basis was measured:
`encoder_ext.rs` and `wels_encoder_ext.rs` carry **zero** fork-reachable bodies,
so with §4.7's MT probes deferred to the session close by the user's direction,
they are the work that needs those probes least and it goes first; B1, stage B's
one MT-seam checkpoint (F217), goes last. The decision was vindicated in the
narrowest possible way: B3 and B2 landed with no aliasing defect at all, and
**both** of the session's aliasing defects (F223) landed in B1.

A session that opens with the layer opens with 38 in-fork bodies at once.

---

## F222 — `safeplan_prohibitions.py` does not skip comment lines, so a writer's name in prose inside an in-fork body is a phantom violation — and it exits non-zero on it

**S3, at the opening baseline, found by running the tool with a writer list one
name longer than S2's.** Prohibition 1 answers "no `&mut self` accessor may be
called from a body whose context parameter is `*mut sWelsEncCtx`". Run with
`pic_mut` in the writer list it reports:

```
prohibition 1: 101 *mut-ctx bodies scanned against 16 writers
  encoder/svc_base_layer_md.rs:1329 WelsMdPSkipEnc (in-fork) calls pic_mut
```

Two separate defects sit behind that one line, and only the second is the tool's:

1. **`pic_mut` does not belong in the writer list.** It is `impl SRefList`
   (`encoder_context.rs:386`), not an accessor on the god-struct — S2's 15 was
   right. Recorded because S2's figure of "15 writers" is quoted with no list
   attached, so the next session has to re-derive which fifteen, and adding a
   plausible sixteenth is what a careful re-derivation looks like. **The writer
   set, for the record**: `func_list_mut`, `mvd_cost_table_mut`, `param_mut`,
   `pps_array_mut`, `rc_at_mut`, `ref_list_mut`, `sps_array_mut`,
   `subset_array_mut`, `vaa_ext_mut`, `vaa_mut`, `ref_list_and_ltr_mut`,
   `vaa_ref_list_and_ltr_mut`, `param_and_paraset_arrays_mut`,
   `param_and_rc_at_mut`, `vaa_and_rc_at_mut`.

2. **The hit is inside a `//` comment**, at `svc_base_layer_md.rs:1343` — prose
   describing where `WelsInitCurrentLayer` stamps a view, quoting
   `(*pRefList).pic_mut(pRefPic)`. The tool's body scan is
   `re.search(r"\b%s\s*\(" % w, body)` over the raw text with no comment
   filter, and it `sys.exit(1)` on any hit.

`safeplan_prohibition2.py` skips comment lines and says why in a line of its own
— `if l.lstrip().startswith("//"): continue   # F178: prose is not a retag`.
Prohibition 1 was written first and never got the same treatment. The
consequence is not hypothetical: this repo's convention is heavy in-body prose
that quotes the exact call spellings under discussion, so the probability that
some future writer's name appears in an in-fork body's comments is high, and the
failure mode is a **red gate with a real-looking violation** pointing at a
comment.

Left as-is by S3 rather than patched, deliberately: the session's edits were to
`src/`, the tool is green for the correct fifteen, and a one-line instrument
change is better landed with a session that can re-run the full battery behind
it. Named here so the next reader does not re-derive it at cost — the fix is
F178's line, copied across.
---

## F223 — B1's storage flip produced **two** aliasing defects in the same field, each invisible to the gate that had just passed, and each caught by exactly one instrument the level below it

**S3, checkpoint B1.** Converting `pVpp` from `*mut CWelsPreProcess` to
`Option<Box<CWelsPreProcess>>` is the plainest kind of ownership change, and it
broke twice. The two failures are worth recording together because they form a
ladder: each was invisible to every gate that had already passed, and each was
refused by the next instrument up.

**Draft 1 — `Option::take`, refused by the Miri encode shards.** Every site
needing the vpp and the context at once did box-out / call / box-back:

```rust
let mut vpp = pCtx.pVpp.take().expect("pVpp lives");
vpp.AnalyzeSpatialPic(pCtx, iCurDid as i32);
pCtx.pVpp = Some(vpp);
```

`gates.sh family` passed it whole: **583/583 byte-identical in both profiles**,
561 debug / 554 release tests, ratchet clean. Miri:

```
trying to retag from <2512223> for SharedReadOnly ... tag does not exist
  <2512223> created by a Unique retag   -> encoder_ext.rs  `pCtx.pVpp = Some(vpp)`
  later invalidated by SharedReadOnly   -> svc_encode_slice.rs `(*pSrcPool).get(id)`
  at WelsSliceHeaderExtInit's `pVpp.as_deref()`
```

`SDqLayer::pSrcPool` stores a raw into `m_pSpatialPicPool` — a **field of the vpp
allocation** — and the fork reads it for a whole frame. The `Box` mints a fresh
`Unique` over the whole block per `as_deref`/store, so the two routes pop each
other in both directions: the shared retag through `pSrcPool` kills the `Box`
tag, and re-storing the `Box` kills `pSrcPool`. This is **F215's rule one
allocation further in** — a `&mut`-shaped route mints per call — and F211's
provenance category is the answer: reach the object by reading the slot as a
*value*, so derivations are siblings off the allocation's own tag. The ownership
moved; the aliasing must not.

**Draft 2 — the slot read, but the wrong half of the pair, refused by the MT
probes.** The fix introduced `ctx_vpp_raw` returning `&mut` off the slot read.
Miri's encode shards then passed (2/2), both sweeps passed again, and the
checkpoint was committed. §4.7's fork/join and mid-row probes — run at the
session close rather than the checkpoint, by the user's direction — refused it:

```
Data race detected between (1) retag write on thread `unnamed-2`
  and (2) retag write of type CWelsPreProcess on thread `unnamed-3`
  (2) -> encoder_context.rs  `&mut *pVpp`   in ctx_vpp_raw
  (1) -> svc_encode_slice.rs `ctx_vpp_ref(pEncCtx).src_id(id)` in WelsSliceHeaderExtInit
```

Two fork-reachable bodies — `WelsSliceHeaderExtInit` and `ctx_pic_ref`, the two
F217 names — were handed the **writer**. A `&mut` retag counts as a *write* to
the data-race model, so N workers each taking one is S63's violation with no read
of the object required to make it real. Both bodies want `src_id(id)`, a `&self`
read. The fix is the reader/writer split the design has mandated since A5, with
`ctx_vpp_ref` (`&`) for the fork and `ctx_vpp_raw` (`&mut`) for the four
single-threaded callers, each verified by `phase9_forksplit.py --why`. The tool
now classifies them apart on its own: `ctx_vpp_ref` in-fork, `ctx_vpp_raw`
ST-flippable.

**The ladder, stated once.** Byte sweeps cannot see a retag that does not change
a byte (F114, F208, F215 — and draft 1). **Single-threaded Miri cannot see a race
between workers** — draft 2 is the first recorded instance of that gap in this
project, and it is the one the plan's §4.7 exists for. The probes are not a
formality on top of the Miri lane; they are the only instrument in the battery
that runs two workers at once.

**The cost of running them late, measured.** Draft 2 shipped in commit
`980bfb16` and was found at the session close, after two further checkpoints'
worth of context had been paged out; the fix had to be re-attributed to B1 and
amended in. Run at B1's own gate it would have been what F215 describes for
A7 — "two silent UB sites into two twenty-minute fixes".

**And the fix itself closes S3 UNVERIFIED by the probes — stated plainly, because
F220's precedent is to name this rather than gloss it.** What draft 3 has:

* the reader/writer split is the design rule (an in-fork body takes the `&self`
  reader), applied at exactly the two bodies F217 independently names;
* `phase9_forksplit.py --why` confirms all four remaining `ctx_vpp_raw` callers
  are **not** fork-reachable, and `--list` now classifies `ctx_vpp_ref` in-fork
  and `ctx_vpp_raw` ST-flippable without being told;
* `gates.sh family` green on the committed tree, and the Miri encode shards green.

What it does **not** have is a completed `fork_join_encodes` run. Five attempts
were made at S3's close and none reached a verdict: two were killed by a
600 s foreground cap mid-compile, one by an over-broad `pkill` of the session's
own making, and the last two — split into concurrent shards, which is the right
pattern and the one `gates.sh`'s Miri lane already uses — were stopped by
direction. The Miri compile is ~12 min per invocation before any test runs
(`[profile.dev] opt-level = 3`, F220's observation), which is what makes every
one of those losses expensive.

**So the standing obligation transfers to S4**, and it is the same one F220
transferred to S3: run `fork_join_encodes` against the tree at `980bfb16`'s
amended tip **before** the first stage-D conversion, so a failure is attributed
to B1 rather than to S4's own work. The expected result is green — the change
removes a `&mut` retag from two fork-reachable bodies and adds nothing — but
that is an argument, and F220's own words apply to it: *that is an argument, not
a run.*

---

## F224 — B1 moves the plan's single progress number **upward**, 612 → 614, by converting four raw fields to owned storage

**S3, checkpoint B1, and it is the honest figure.** §9 makes
`#[allow(unsafe_code)]` outside `src/api/` the plan's one progress number. B1
converts `pOut`, `pVpp` and `pSliceThreading` to `Option<Box<T>>` and
`mutexSliceNumUpdate` to an owned `Mutex<()>`, deletes `WelsMutexInit` and
`WelsMutexDestroy` outright, and makes `with_wels_mutex` a safe fn. Counted per
file across the checkpoint:

```
encoder_context.rs        25 -> 29    (+4: ctx_out_raw, ctx_src_pool_raw, and the
                                           ctx_vpp_raw / ctx_vpp_ref pair F223 forced)
slice_multi_threading.rs  28 -> 26    (-2: WelsMutexInit and WelsMutexDestroy deleted,
                                           with_wels_mutex now safe, ctx_slice_threading_raw added)
                                       net +2
```

**Why the metric scores it backwards.** A raw *field* carries no `#[allow]` —
`pub pOut: *mut SWelsEncoderOutput` was invisible to the count while being
exactly the thing the plan exists to remove. What the count *can* see is the
aliasing contract those fields implied, which is now written down as four named
accessors with a documented reason each. So the checkpoint traded four unnamed,
uncounted raw fields for four named, counted, audited derivations — and the
number rose.

This is **F214's finding running in the opposite direction**. F214 recorded that
A6's flip "moves nothing the ratchet counts"; here the work moves it the wrong
way. Both are the same underlying fact: the metric counts *annotations*, and
structural work changes what needs annotating in ways that do not track progress.
F216 already warned the figure "should be read before it is quoted at anyone";
S3 adds that it can go **up** on a checkpoint that strictly reduces raw exposure,
and that a session judged on it alone would be judged wrongly.

The figure S3 does not dispute: it opened at 612 (`safeplan_tracking.sh`,
matching S2's close exactly) and closes at **614**. Two of the four added lines
are the `ctx_vpp_raw`/`ctx_vpp_ref` pair, which exists because F223's data race
forced the reader and the writer apart — so one of the two is, precisely, the
price of the soundness fix the MT probes bought.


---

## F225 — the fork pair is GREEN on B1's fix, and the brief's "you will produce the first" is wrong: four prior pairs are already recorded, in prose, with no tripwire

**S4 step 0, the duty S3 handed forward.** F223 closes with B1's second-draft
fix — the `ctx_vpp_ref`/`ctx_vpp_raw` reader/writer split — backed by static
classification and by `family`, but never by the probes that found the defect it
answers. F220 handed that obligation from S2 to S3; S3's own close handed it here.
It is now discharged. Run against `410b9c13` (the tree as S4 found it, before any
conversion), one compile-only pass then the two probes concurrently:

```
compile rc=0 wall=5s          (warm target/miri)
fork_join_encodes_a_multi_slice_frame_..._aliasing_checker   ok  3463.45s
fork_join_encodes_a_frame_whose_slice_boundary_is_mid_row    ok  3530.38s
pair wall 3535 s = 59 min;  zero UB, zero data races in either log
```

Both GREEN. B1's fix is verified by the instrument that refused its predecessor,
and F223's open tail is closed.

**The correction, and it is the fourth session in a row that a count reached a
brief without its command (F221's rule).** S4's brief says, of these two probes:

> The `fork_join_encodes` probes have **no completed measurement** — S2 never got
> one either. You will produce the first.

They have four. `tools/miri_wall_baseline.txt` records, in its own prose:
T9.E8 `3356 / 3449` ("first green"), T9.E2 `3294.86 / 3343.20`, T9.H `3411.72 /
3493.33` parallel, and T9.H `3194.20 / 3260.15` serial. `gates.sh`'s own header
says the same thing in the line that explains why the lane skips them — "their
first green runs measured 3356 s and 3449 s". What was true is narrower and worth
stating in the form the brief wanted: this is the first completed pair **of the
safe-conversion plan** (S1–S4 had none), and the first that verifies any
conversion checkpoint. Against the most recent comparable form the ratios are
**1.015 / 1.011 per probe and 1.010 on the pair wall** — the flat series T9.H
already measured across the context flip, and nowhere near S61's 1.3x tripwire.

**Why the miss was structurally likely, and what is done about it.** Those four
pairs live in *prose* — a commented regime table inside a file whose machine-read
data line is the `session` lane's number and has never carried a fork figure. So
the numbers were simultaneously recorded and unreadable: no reader parsed them, no
tripwire compared against them, and a brief could truthfully say the file's data
line held nothing for these probes. S58's rule is that a silent gap reads as a
clean run, and this is that shape one level up — a *measured* quantity with no
instrument pointed at it reads as an unmeasured one.

Seeded, therefore, as its own instrument rather than as a fifth paragraph of
prose: **`tools/fork_join_baseline.txt`** carries the format, the four prior pairs
in both scheduling forms, and this run as its data line;
**`tools/fork_join_probe.sh`** runs the pair in the prescribed parallel form
(one zero-match compile pass, then two concurrent invocations — never
`--no-run`, which reports up-to-date without building the interpreted target)
and applies the S61/F140 comparison, warning past 1.3x and never failing. It
corroborates each rc against its parsed totals and fails loudly on a probe that
ran **zero** tests, per F17 — a renamed probe must not pass by vanishing, which
is the failure mode a filter-driven runner is most exposed to.

Two limits, stated rather than left for the next reader to find. The comparison is
**wall only**: the pair runs as two separate `cargo` invocations rather than inside
one shell's process tree, so the `times`-builtin CPU measurement F170 added to the
lane does not reach them, and wall here is load- and suspend-sensitive exactly as
F170 warns. Read a ratio past the tripwire as "re-measure on a quiet machine"
before reading it as a codec regression. And the runner is **not wired into
`gates.sh`**: at ~59 minutes the pair cannot join a 20-minute level, and
`full`/`exit` already run the probes their own way — so this is the explicit-command
path §4.7 asks for, one command instead of a remembered two-line incantation.

**Three mechanics, measured, for whoever runs it next.** The compile is **5 s** on
a warm `target/miri`, not the ~12 min the brief prices — that figure is a cold
build, and it is what makes "compile once, then shard" cheap rather than
essential. A foreground run dies at the harness's 600 s cap mid-compile, which
cost S3 ~25 min twice; background it. And do not `pkill -f` on a pattern that
matches your own runner: S3 killed its one healthy run that way, which is why
`fork_join_probe.sh` says so in its header.

---

## F226 — `UpdateMbListNeighborParallel` takes a whole-`SSliceCtx` `&mut` for one scalar read, and N workers take it at once — F223's third rule already broken in the tree, on the one fork no gate reaches

**S4, found by D2's classify-before-convert pass, before any conversion** — which
is the protocol step earning its keep rather than a lucky grep. The brief asks for
every `*mut SDqLayer` body to be tabulated by whether it writes *through* the
layer. Three of forty do. Reading the first of them found this
(`slice_multi_threading.rs`, `UpdateMbListNeighborParallel`):

```rust
let pSliceCtx = &mut (*pCurDq).sSliceEncCtx;
let kiMbWidth = pSliceCtx.iMbWidth as i32;
```

The binding is used for **exactly one read** of a frame-constant `i16` and is
never written through. But `UpdateMbMapForked` spawns `ForkWidth(..)` workers
that all call this body on the **same layer**, one call per slice index, inside a
`std::thread::scope`. F223's third rule is exact here: in-fork, a `&mut` retag is
a *write* to the data-race model. This one is a `Unique` retag over the whole
`SSliceCtx`, which spans `iSliceNumInFrame` (an `AtomicI32`) and the
`pOverallMbMap` `Vec` header — so N workers each taking it is S63's violation with
no read of the object required to make it real. It is F223's defect 2 in a
different struct: two workers, a gratuitous exclusive reborrow, nothing written.

**Why no instrument in the project can see it, and this is a three-layer
blindness rather than an oversight:**

1. **Byte sweeps.** The retag changes no byte, and the path needs
   `bUseLoadBalancing`, which **both diffharness drivers pin off** — the covering
   test's own doc calls this path the project's second expected-divergent class
   (its slice boundaries are a function of wall-clock encode times, so two runs of
   the *C++* disagree with each other). No sweep row can ever reach it.
2. **The §4.7 MT probes.** Both force `bUseLoadBalancing` off — the multi-slice
   probe says so in its own doc, because that is what makes its boundary
   assertions mean anything. So `UpdateMbMapForked`'s fork never runs under them.
3. **The one test that does drive the path**,
   `load_balancing_completes_frames_with_sane_slice_counts`, is
   `#[cfg_attr(miri, ignore)]` — 192 macroblocks x 4 frames x 4 threads, priced in
   its own doc at ~8x the fork probe, which at this session's measured 59-minute
   pair (F225) is something like eight hours. Natively a retag is not an
   instruction, so nothing fails.

That test's doc states the justification for the exemption as: "The aliasing
question this path raises is the fork/join's, and that probe answers it." **That
sentence is false for this site**, and it is the load-bearing claim — the fork
probe forces the flag off and therefore never enters this fork at all. The
exemption is right about cost and wrong about coverage.

**The fix is one word** — read the scalar through the raw and form no reference:

```rust
let kiMbWidth = (*pCurDq).sSliceEncCtx.iMbWidth as i32;
```

**The instrument that should back it is not the encoder.** The covering test is
unaffordable under Miri because it drives a whole encode; the aliasing question
does not need one. `layer_with_bank` already builds a hand-made `SDqLayer` for two
tests in this file — give it a real `sMbDataP` grid and slice tables, then two
scoped threads each calling `UpdateMbListNeighborParallel` on its own slice index.
That is the exact concurrency `UpdateMbMapForked` creates, at a geometry Miri can
afford. Under Miri it refuses the old line as a data race and passes the new one;
natively it is a neighbour-map correctness test.

**Both landed in S4, and the probe was checked for teeth rather than assumed to
have them.** `update_mb_map_forked_workers_share_the_layer_without_racing` builds
a 4x2 grid in two slices, gives each record its coordinates, and spawns one scoped
thread per slice — `UpdateMbMapForked`'s shape with the encoder removed, because
the aliasing question is about two workers and one layer and never needed an
encode. Run against the **restored** `&mut` binding it fails exactly as F223's
defect did:

```
Data race detected between (1) retag write on thread `unnamed-2` and
  (2) retag write of type `SSliceCtx` on thread `unnamed-3` at alloc296935+0x140
  --> slice_multi_threading.rs:463   let pSliceCtx = &mut (*pCurDq).sSliceEncCtx;
```

and against the fix it passes in **1.23 s**. That number is the point: the test
this replaces for this question is priced at ~8x the fork pair, which at S4's
measured 59-minute pair (F225) is something like eight hours, and it is
`#[cfg_attr(miri, ignore)]` for exactly that reason. A probe that drops the encode
buys the same verdict for a second.

It also **joins the session battery**: the name is under `encoder::` and matches
neither of the Miri lane's two skips, so `gates.sh session` runs it in the `enc`
shard from here on. The blindness this finding records is therefore closed at
session cadence rather than by remembering to run something.

**Scope of the surrounding classification, since it is what turned this up.** Of
the 40 bodies carrying a `*mut SDqLayer`, 37 only read the layer struct; the other
two writers (`ReallocateSliceList`, `ReallocateSliceInThread`) write
`sSliceBufferInfo[slot]` for a bank T7.B2 made this worker's own, so their `&mut`
projections cover sibling ranges rather than the shared struct — lawful, and
unlike this one they are *used* for the write they take.

---

## F227 — C4a's `&mut [u16]` conversion took `as_mut_ptr()` **twice**, and the second call popped the first cursor's tag: F215's rule one container in, caught by the gate level above the one the checkpoint ran

**S4, found by `gates.sh session` on the checkpoint *after* the one that
introduced it.** C4a converted `MvdCostInit` from `(pMvdCostInter: *mut u16)` to
`(pMvdCostInter: &mut [u16])` so the table's extent would travel with it. The body
keeps two interleaved raw cursors — the C++ fills 52 QP rows from both ends at once
— and the conversion derived them the obvious way:

```rust
let mut pNegMvd = pMvdCostInter.as_mut_ptr();
let mut pPosMvd = pMvdCostInter.as_mut_ptr().offset((kiSz + 1) as isize);
```

`as_mut_ptr` takes `&mut self`. So the second call is a **fresh `Unique` retag over
the whole slice**, and it pops the tag `pNegMvd` is still holding; the next write
through `pNegMvd` is a write with a dead tag. Miri:

```
Undefined Behavior: attempting a write access using <654466> at alloc296457[0x0],
  but that tag does not exist in the borrow stack for this location
  --> md.rs:1946   *pNegMvd = kiLambda.wrapping_mul(BsSizeSE(iNegSe) as u16);
  <654466> was created by a SharedReadWrite retag at offsets [0x0..0x1aafa]
```

**This is F215 exactly, one container in.** F215 says a `&mut self` *accessor* on
the context is a fresh whole-struct `Unique` per call, so two raw cursors may only
coexist if they come off the same call. The identical rule holds for `&mut [T]`
and `as_mut_ptr`, and the conversion that introduces a slice parameter is precisely
where it becomes easy to write two derivations without noticing — the old code took
its two cursors off one raw parameter, where the question could not arise. The fix
is one line: take the root once, and let both cursors be siblings of it.

**What this says about gate levels, and it is the finding's point.** C4a was gated
at `family`, per the S4 brief's rule that stage C runs `family` per checkpoint and
reserves `session` for the seam files. `family` passed it whole — **sweeps 583/583
byte-identical in both profiles**, 562 debug / 555 release, ratchet clean, census
clean — because the defect changes no byte: both cursors compute the same addresses
and write the same values, and Stacked Borrows is the only observer that objects.
`family` does not run Miri. The defect survived a commit and was caught by the next
checkpoint's `session` gate.

So the brief's split is wrong in one specific way, and the correction is narrow:
**a checkpoint that converts a raw parameter into a slice or reference is a Miri
question regardless of which file it is in.** It is not the seam that makes Miri
necessary — the seam is what makes the *MT probes* necessary. Byte gates cannot see
a retag, which is F223's first rung, and F223 recorded it for the fork; this is the
same rung reached single-threaded. Any checkpoint of this shape runs `session`.

Two smaller notes worth keeping. The failure surfaced as **four Miri shards
reporting `0 passed / 0 failed, rc=1`** rather than as a named test failure — the
UB aborts the process, so the shard produces no totals. `gates.sh` fails loudly on
a shard that ran zero tests (its own F17 rule, written for renamed probes) and that
is what turned an unreadable rc into a legible verdict; without it the run would
have looked like a filter that matched nothing. And the cost of catching it late
was small only because the fix is one line: the checkpoint had already landed, so
the correction rides in the following commit with this finding attached, rather
than being amendable into the commit that caused it.

## F228 — a `&self` context accessor is a *whole-context* retag, and inside the fork that races any worker's inline-field write; the raw accessors got away with it only because their borrows died on the next line

**Where.** `sWelsEncCtx::mvd_cost_origin` (retired at S5.C4b), and by construction
every other `&self` accessor on the context that a fork-reachable body calls.

**What was believed.** The accessor family's own header says the conversion from
`addr_of!` to `&self` + `as_ptr` "is the same sibling-stable derivation", and
`every_container_accessor_hands_out_sibling_cursors` asserts exactly that —
single-threaded. Nothing in the family's documentation distinguishes a `&self`
derivation from a field-precise one, and C4b's first design took `&self` for
granted on that basis.

**What is true.** `&self` on `sWelsEncCtx` retags the **whole** context allocation.
Under the fork that retag is a *read* of every inline field, so it races any other
worker's concurrent write to any of them. A field place projection through the raw
pointer — `&(*p).pMvdCostTable` — retags that field's bytes and nothing else, and
does not race. Both halves measured directly, two scoped threads, no encoder:

* whole-context: `Data race detected between (1) non-atomic write on thread
  unnamed-2 and (2) retag read of type sWelsEncCtx on thread unnamed-3`
* field-precise: passes, including when the derived `&[u16]` is *held* across the
  other worker's writes for the whole of the worker's body.

**Why nothing has caught it.** `mvd_cost_origin` returned a `*mut u16`, so its
`&self` borrow lived for the length of the call and died on the next line. The
window was one call wide, and no probe ever landed a concurrent inline write
inside it. C4b's conversion widens that window to the entire macroblock loop —
which is what turned a latent shape into one worth measuring.

**What C4b did with it.** `MvdCostCursor::origin` takes the table as a `&[u16]`
the *caller* derives field-precisely, and its header says so as a requirement
rather than a preference. The referee is
`mvd_cursor_survives_a_slice_held_across_the_forked_workers`
(`svc_encode_slice.rs`, ~1.3 s under Miri), whose teeth are checked: respelling
the derivation whole-context reproduces the race above.

**Standing consequence, not acted on here.** Every remaining `&self` accessor on
`sWelsEncCtx` called from a fork-reachable body carries the same shape. They are
sound today for `mvd_cost_origin`'s reason — short borrow windows — and that is an
argument about timing, not about types. `svc_encode_slice.rs`'s
`(*pEncCtx).func_list()` clusters, scoped one use-cluster at a time with the "A6"
comments, are the same reasoning already applied by hand. A sweep that converts
the fork-reachable ones to field-precise projections is available and unowned.

**Correction to S5's brief.** The brief says the DQ-layer flip needs storage moved
first because "a whole-struct shared borrow racing those writes is undefined
behavior under Miri's model". That is right, and F228 is the same sentence about
the *context* rather than the layer — the brief states the rule for `SDqLayer`
only, and the property is not the layer's, it is every struct the fork shares.

## F229 — C5's premise is not "dark code", it is *unreachable* code: nothing in the port can allocate `SScreenBlockFeatureStorage`, so converting its fields would invent the ownership shape T7.C6 deliberately declined to invent

**What S5's brief says.** C5 is `SScreenBlockFeatureStorage`'s five raw buffer
pointers, to be converted "to `Vec` + index tables, along with their mirrors in the
motion-estimate code", and the brief warns that the path is dark: "the
screen-content path installs only under `bScreenContent && bEnableSceneChangeDetect
&& iComplexityMode < HIGH`, and no harness driver ever sets `bScreenContent`".

**That is a claim about configuration. The truth is structural, and stronger.**
Measured at this tree:

* `SPicture::pScreenBlockFeatureStorage` is **never assigned** — anywhere in `src/`,
  `tests/` or `benches/`. Its only writer would be `AllocPicture`, which *refuses*
  `iNeedFeatureStorage != 0` and returns `None`, and all three of its call sites
  pass `0` or `false`. The field is null for the life of every picture the port
  can build, under every configuration, including ones no harness selects.
* `PerformFMEPreprocess` — the only writer of `pFeatureOfBlockPointer` and the only
  caller of `CalculateFeatureOfBlock` — is **declared and never called**. Zero call
  sites in the whole repository.
* `CalculateFeatureOfBlock` is the sole caller of the three dispatch slots the
  family turns on (`pfCalculateBlockFeatureOfFrame`, `pfInitializeHashforFeature`,
  `pfFillQpelLocationByFeatureValue`). The slots are installed in `WelsInitMeFunc`'s
  `bScreenContent` arm and then reached by nothing.
* The read side (`SetFeatureSearchIn`, `FeatureSearchOne`) is gated on
  `SWelsME::pRefFeatureStorage`, which `layer_ref_feature_storage` resolves from the
  same never-assigned field.

So the family is not dormant behind a flag — it is unreachable behind a deleted
allocator, and no flag can reach it.

**Why that stops the checkpoint rather than merely colouring it.** The four
allocations were transliterated once and then deleted at T7.C6, whose note states
the reason in terms this checkpoint inherits verbatim: *"Deleted rather than
converted, and the difference matters. Converting them would have produced four
owned buffers behind a struct whose fields (`SScreenBlockFeatureStorage`'s raw
pointers) are Phase 10's to design — a shape nobody has decided, on a path nobody
runs."* C5 asks for exactly the conversion that note declined, one level in: the
fields rather than the allocations. The design question is the same one, and it was
already answered in the opposite direction by a session that had measured it.

**And the shape is genuinely undecided, not merely undesigned.** `pLocationOfFeature`
and `pFeatureValuePointerList` are arrays of pointers *into* `pLocationPointer`'s
arena, so the safe form is an index table — but `pFeatureOfBlockPointer` is a buffer
the storage does **not** own: `PerformFMEPreprocess` takes it as a parameter and
stores it. Converting the struct to own `Vec`s therefore gets one field wrong, and
converting it to hold indices requires naming an owner that does not exist in this
tree. Whichever is chosen, Phase 10 inherits it.

**What it would have bought.** Two `#[allow(unsafe_code)]` —
`InitializeHashforFeature_c` and `FillQpelLocationByFeatureValue_c` — and only if
their two `unsafe extern "C"` fn-pointer typedefs in `SWelsFuncPtrList` are
converted with them. Everything else in the family stays `unsafe` on
`*mut SScreenBlockFeatureStorage` or on the raw reference plane the frame-feature
builders walk, neither of which is C5's. C5 does not move `svc_motion_estimate.rs`
or `picture.rs` towards `forbid(unsafe_code)`, because `SWelsME::pRefFeatureStorage`,
the union read of `uSadPredISatd` and the raw plane kernels all remain.

**The referee C5 was given cannot see a mistake.** The brief's instruction — "Review
the diff as the only referee it will get" — is accurate and is the problem: no
sweep row, no unit test, no Miri probe and no gate executes a single line of this
family, so a semantic error in the conversion is undetectable by every instrument
this project has, and would sit in the tree until Phase 10 ran it.

**Amendment (S5, after the ruling was put): deletion was considered and refused, on
a fact neither T7.C6 nor this finding had stated.** The obvious alternative to
converting unreachable code is deleting it, and the prize is real — the whole
screen-content feature-search subtree is 15 functions, **538 lines and 13 allows** in
`svc_motion_estimate.rs` plus the 12-line struct, which is 13 of that file's 22 and the
difference between it being convertible and not.

It is refused because the family does not separate where it looks like it does. The
*storage* is unreachable, because nothing allocates it. Its *readers* are not:
`SetFeatureSearchIn`, `FeatureSearchOne` and `MotionEstimateFeatureFullSearch` are
called from `WelsDiamondCrossSearch`, which `WelsInitMeFunc` installs into
`pfSearchMethod` in its `bScreenContent` arm — and `bScreenContent` is
`iUsageType == SCREEN_CONTENT_REAL_TIME` (`param_svc.rs:632`), **a public API parameter
any caller can set**. So today a `SCREEN_CONTENT_REAL_TIME` caller gets the
screen-content search methods installed and running, with the null guards bailing on
the storage that was never allocated: degraded, but its own behaviour. Delete the
family and that caller silently gets camera-content search instead.

That is a behaviour change on a live API configuration, and **no instrument in this
project can see it** — no harness driver sets that usage type, so the sweep, the gtest
smoke and every Miri probe are all blind to it, exactly as they are to the conversion
this finding declines. Deleting is therefore the *more* dangerous of the two options,
not the safer one, which is the opposite of how it reads.

This is also the sentence T7.C6's note was missing. It deleted the allocations because
they were unreachable *and* unobservable — eleven and thirty lines of allocate-and-null
that no configuration could reach. The search tree is only unreachable *by
configuration*, and the boundary it drew at "the struct and its fields are untouched"
turns out to be exactly the line between the two.

**Stopped, not skipped, and this is the ruling asked for.** Per S5's rule — "When a
blocker needs the user's ruling, write the finding and stop that checkpoint rather
than guessing" — C5 is left undone and named in the roll-forward. The decision it
needs is one sentence: either the family is converted now, accepting that Phase 10
inherits an ownership shape chosen without a running caller, or it stays raw until
Phase 10 ports it whole from the reference, as T7.C6 ruled for its allocations. This
session took neither side; it measured the ground and stopped.

## F230 — the de-unsafe cascade diverges if the `unsafe` keyword and the `unsafe {}` blocks are stripped together; the pass that converges strips only declarations whose signature carries no raw pointer

**What S5's brief promises.** "The compiler-driven fixpoint (strip, read the errors,
revert what fails, iterate) recently retired 87 signatures in one pass; rerun it after
your conversions land — the de-unsafe cascade is where most of the tracking number's
movement comes from." It does not say what to strip, and the obvious reading — strip
everything that mentions `unsafe`, then restore what the compiler names — **does not
terminate**.

**Measured.** Stripping all three at once (492 `#[allow(unsafe_code)]`, 143 `unsafe fn`
keywords, 166 `unsafe {` openers) and restoring at each error's enclosing declaration
ran: 1976 errors → 217 → 191 → 217 → 279 → 340 → 404 → 459. It diverges after round 3
and the reason is structural, not a bug in the restorer: restoring `unsafe` on a
function makes every *call* to it an error, because the callers' `unsafe {}` blocks
were stripped in the same pass. Each restore manufactures new errors one level out, so
the frontier expands instead of closing. The run was discarded (`git checkout`), and
the tracking number it left behind — 611 against a starting 501 — is what a diverged
restore looks like.

**The pass that converges strips one thing.** Take the `unsafe` keyword off `fn`
*declarations*, and only where the signature carries **no raw pointer**; leave every
block and every allow exactly where it is. Nothing then cascades to call sites: a
caller's `unsafe {}` around a now-safe call is at worst an `unused_unsafe` warning. And
the compiler is a two-sided oracle for the ones that must keep it, because `src/lib.rs`
allows `unsafe_op_in_unsafe_fn` — an `unsafe fn` body *is* an unsafe context, so taking
the keyword away turns every genuinely unsafe operation in that body into an E0133, and
a body installed in an `unsafe fn` pointer slot into a type mismatch. That pass ran
994 → 76 → 8 → 0 and reached its fixpoint in four rounds, twice, identically.

**The raw-pointer filter is the part that makes it sound, not just convergent.** A
declaration whose signature has a `*mut`/`*const` may carry a real precondition, and
this pass does not touch it. What is left — a signature of references only — cannot,
because the `# Safety` clauses on them say things like "`pCtx` must be a live encoder
context", which a `&mut sWelsEncCtx` guarantees by construction. Of the 26 retired,
every clause read that way; `InitFunctionPointers`'s adds "whose parameters are built",
which is a correctness precondition whose violation is a panic, not undefined behavior,
and so was never `unsafe`'s to state.

**Second-order rule, learned twice in this pass.** Restoring or removing an annotation
must be done at the enclosing **item**, never at the error's line. A restorer that
inserts one `#[allow]` per *error* produces several per function and inflates the count
(501 → 515 on the first attempt), and one that inserts into a macro body produces
`error[E0658]: attributes on expressions are experimental` in the three files that have
them (`encoder_context.rs`, `encoder/picture.rs`, `decoder/picture.rs`). Both were
discarded and redone item-wise.

## F231 — 70 `# Safety` clauses document preconditions for pointers their functions no longer take

Found while auditing F230's 26 retirements for real contracts, which is the check that
should precede any de-unsafing: `au_set::WelsWriteSpsSyntax` still says "`pSps` and
`pBsWriter` must be non-null; `pSpsIdDelta` must point to an array indexable by
`pSps->uiSpsId`", and its signature has been `&mut [u8]`, `&SWelsSPS`, `&mut BsWriter`,
`&[i32]` for some sessions now. Not one of those claims is about anything that still
exists: a reference is non-null by construction, and the array claim is a bounds
question `&[i32]` answers with a panic.

Swept: **70** `# Safety` clauses sit on functions that are safe `fn` today, across
`get_intra_predictor.rs` (25 — the whole intra-prediction kernel family),
`paraset_strategy.rs` (5), `au_set.rs` (6), `rc.rs` (5), `encoder_ext.rs` (3),
`svc_enc_slice_segment.rs` (3), `decode_slice.rs` (3) and fourteen others. This session
cleaned only the ones it created (5 files); the remaining ~55 predate it and are left
named rather than silently swept, because the clean is mechanical but the *reading* is
not — each one should be checked for a surviving precondition before its clause is
deleted, exactly as the 26 here were.

The reason it matters beyond tidiness: a stale `# Safety` clause is the evidence a
future session will use to decide whether a signature may be de-unsafed. F230's pass
consults precisely this text. Left in place, these clauses argue for keeping `unsafe`
on functions that have not needed it for several phases.

## F232 — the remaining 472 allows, by what actually blocks each one: 197 have no raw parameter at all, and `current_layer` is the single edge holding a whole file

Measured at S5's close, classifying every `#[allow(unsafe_code)]` outside `src/api/`
by the *signature of the item it heads*:

| blocked by | count |
|---|---|
| **no raw parameter at all** — the item's signature is references and scalars | **197** |
| some other raw parameter (planes, DCT blocks, C-ABI structs, `BsWriter`, …) | 124 |
| `*mut sWelsEncCtx` — the fork's context pointer | 114 |
| `*mut SDqLayer` | 29 |
| not a function (an `impl`, a `static`, a test body) | 8 |

**Read the first row carefully, because it is not 197 free wins.** S5's E2b pass took
the ones whose *bodies* were also clean and got 26. The rest have raw pointers inside,
obtained from raw accessors they call. `ref_list_mgr_svc.rs` is the clearest case and
worth stating exactly: **31 allows, every one of them on a signature with no raw
pointer**, and stripping the file's `unsafe` keywords produces 101 errors — 51 raw
dereferences in the bodies plus calls to `current_layer` (17), `SPicture::SetUnref`
(11), `ctx_param_raw` (8), `ctx_vpp_raw` (5) and `slice_in_layer` (4). Not one of those
31 is blocked by its own contract; they are blocked by four accessors that still hand
back pointers. `current_layer` is the largest single edge in the graph, and F210
deferred it to D2–D3 precisely because the fork *writes* the layer.

**Correction to a number this session first published.** An earlier pass of this
census reported 242 allows blocked on `*mut sWelsEncCtx` and 57% blocked on the two
fork pointers. That was wrong: the classifier matched the *string* `sWelsEncCtx` and so
counted `&mut sWelsEncCtx` — a reference — as a raw pointer. `rc.rs` has 10 raw-ctx
functions, not 37. The corrected figures are the table above: **143 of 472 (30%)** are
held by the two fork-shared pointers, not 57%. The difference matters for planning,
because it is the difference between "most of what is left needs the fork seam
redesigned" and "most of what is left is reachable without touching it".

**What this says about the plan's stage ordering.** D2/D3 is scheduled as the
centrepiece and `svc_encode_slice.rs` is its subject — but that file's 72 allows are
38 ctx + 15 layer, so **74% of it is behind the two pointers** and cannot move until
the DQ-layer storage moves (the brief's own prerequisite: banks out of the struct,
`NumSliceCodedOfPartition`/`LastCodedMbIdxOfPartition` to atomics). The files with
**zero** ctx dependency and real convertible surface are, in order:
`encoder_ext.rs` (32: 16 raw, 16 body), `ref_list_mgr_svc.rs` (31, all body, one edge),
`wels_encoder_ext.rs` (24: 17 raw), `wels_preprocess.rs` (26: 14 raw),
`svc_motion_estimate.rs` (22), `svc_enc_slice_segment.rs` (21), `encode_mb_aux.rs` (12)
and `nal_encap.rs` (11). That is **179 allows reachable without the fork seam**, which
is where a session that wants movement should go before it takes on the layer.

## F233 — `SetFeatureSearchIn` dereferences a null `pRefFeatureStorage` on an API-reachable path, and F229's dead allocator is why the pointer is always null

Found while converting the pointer, not by looking for it.

`SetFeatureSearchIn` reads `(*pRefFeatureStorage).pTimesOfFeatureValue` and
`.pLocationOfFeature` with **no null test**, where its sibling
`CalculateFeatureOfBlock` guards all four of its pointers and returns `false`. The
argument comes from `SWelsME::pRefFeatureStorage`, which `InitMe` copies from
`layer_ref_feature_storage`, which resolves
`SPicture::pScreenBlockFeatureStorage` — **never assigned anywhere in the tree**
(F229). So the pointer is null on every path.

Reaching the read needs `bScreenContent`, which is
`iUsageType == SCREEN_CONTENT_REAL_TIME` (`param_svc.rs:632`) — a public API
parameter, not a harness knob. `WelsInitMeFunc`'s screen arm installs
`WelsDiamondCrossFeatureSearch`, whose body calls `SetFeatureSearchIn` once
`uiSadCost >= uiSadCostThreshold`. A caller setting that usage type therefore reaches a
null dereference. It has never been observed because **no driver in this project sets
it** — the same blindness F229 records for the conversion it declines, showing up here
as an actual defect rather than a hypothetical.

**Fixed by the type, at the point of the read.** With
`pRefFeatureStorage: Option<&SScreenBlockFeatureStorage>`, the site becomes
`let Some(..) = .. else { return false; }` — the answer its sibling already gives. The
guard sits at the dereference rather than at the top of the function deliberately:
every line above it still runs exactly as before, so the only behaviour that changes is
the behaviour that was undefined.

**Note what this says about F229's ruling.** The finding argued that *deleting* the
screen-content family is dangerous because the search tree is API-reachable and no gate
can see it. This is the same fact from the other side: that path is reachable enough to
contain a live null dereference. Both conclusions stand together — the code must not be
deleted blind, and it must not be left unexamined either.

## F234 — a two-thread Miri race probe with too few rounds is not a weak referee, it is a blind one that reads as a passing test

S5's brief makes targeted Miri probes the referee for every multi-threading seam:
"build the data structure by hand *without* an encoder, spawn two threads doing exactly
what the workers do, and let Miri judge." It does not say how long they must run, and
the answer is not "long enough to be thorough" — it is **long enough for Miri's
scheduler to interleave the two accesses at all**.

Measured while building D2a's probe, on the probe's own control:

| rounds | non-atomic control (the teeth check) |
|---|---|
| 8   | **green** — silent |
| 200 | red: "Data race detected between (1) non-atomic write on thread `unnamed-2` and (2) retag read of type `SDqLayer` on thread `unnamed-3`" |

Same code, same flags, same two threads; only the loop count differs. Miri's data-race
detector reports races in the schedule it actually runs, and its default preemption rate
is low, so two short worker bodies can run almost to completion without the interleaving
that exposes the conflict. Raising `-Zmiri-preemption-rate` to 0.5 did not change the
8-round verdict either — **rounds, not preemption, was the variable that mattered here**.

**Why this is worse than a weak test.** The 8-round probe passed with the atomics in
place *and* passed with them taken away. Had it been written that way and committed, it
would have sat in the Miri lane as a green referee for a property it could not observe,
and the next session would have read its passing status as evidence. That is the same
failure mode as a filter that matches no tests — which `gates.sh` already guards against
with its own F17 rule for renamed probes — arriving through a different door.

**The rule this fixes.** A probe of this shape is not finished when it passes. It is
finished when its *control* — the same probe with the property deliberately broken —
has been run and seen to fail. D2a's probe carries the round count as a documented
constant with this reason attached, so that a later reader trimming it "because 200
seems excessive" is told what 200 is buying.

The two probes this session added were both teeth-checked before being relied on:
C4b's (`mvd_cursor_survives_a_slice_held_across_the_forked_workers`, red on the
whole-context spelling) and D2a's (red on the non-atomic field). C4b's happened to be
sharp at 8 rounds because its control conflicts on every iteration; D2a's does not, and
nothing about the two probes' shape says which is which. Check the control.

## F235 — the DQ layer's two obstacles need *different* fixes, and the Miri message says which: "non-atomic write" wants atomics, "retag write" wants a separate allocation

S5's brief prescribes both halves of the DQ-layer storage move in one breath — "the
banks to their own allocation outside the struct, the counters to atomics" — without
saying why the two get different treatment. The probes make the reason legible, and it
is worth stating because the next such field will present the same choice.

Both obstacles are inline fields of `SDqLayer` written from inside the fork, and both
make a sibling's whole-layer `&SDqLayer` retag illegal. But Miri diagnoses them
differently, because the conflicting access is different:

| field | in-fork writer | Miri's report on the control | fix |
|---|---|---|---|
| `NumSliceCodedOfPartition`, `LastCodedMbIdxOfPartition` | `[kiPartitionId] = ..` / `+= 1` — a plain store | "(1) **non-atomic write** … (2) retag read of type `SDqLayer`" | atomics |
| `sSliceBufferInfo` | `&mut ..[kiBank].pSliceBuffer` — a borrow | "(1) **retag write** … (2) retag read of type `SDqLayer`" | separate allocation |

Atomics fix the first because the conflict *is* the write, and an atomic write is not a
data race against a concurrent read. They cannot fix the second: the conflicting access
there is the `&mut` **retag**, which happens whatever the field's type is — making
`iMaxSliceNum` atomic would leave `&mut ..pSliceBuffer` retagging the same bytes.
Moving the banks to their own allocation fixes it because a retag is confined to the
allocation it names, so a `&mut` into the box cannot conflict with a `&` over the layer.
That is F163's argument, the one C4b relies on for the MVD table.

**A detail that decides whether a probe tests anything.** `Box<[T; N]>` works here only
because rustc handles `Box` place-deref natively: `&mut ..sSliceBufferInfo[w]` creates
no `&mut Box<..>`, so the eight header bytes that *do* remain inline are never retagged.
A probe written with `addr_of_mut!` instead of `&mut` would pass without exercising any
of this — it creates no reference at all. D2b's probe uses `ReallocateSliceList`'s exact
spelling for that reason, and its control was re-run after the spelling changed.

**Measured outcome.** With both fixes in, a scan of every write and every `&mut` through
a raw layer pointer, cross-referenced against `phase9_forksplit.py`, reports **zero**
`SDqLayer` fields written from inside the fork — `ReallocateSliceList`'s bank borrow is
the one remaining in-fork `&mut`, and it now lands in the box. `sMbDataP` was never an
obstacle: `MbArray<T>` is `Vec`-backed, so per-worker macroblock writes already went to
a separate allocation.

## F236 — the `&SDqLayer` flip is unsound as a straight substitution, because `current_layer` can return null and ten bodies' null guards are load-bearing; here is the split it actually needs

D2a and D2b removed both reasons a body could not *hold* `&SDqLayer` — no field of the
layer is written from inside the fork any more. S5 then attempted the flip itself
(D2c) and **reverted it**. The obstacle is not aliasing; it is nullability, and it is
worth writing down because the naive version compiles.

**The hazard.** `current_layer` returns `*mut SDqLayer` and answers **null** whenever
`iCurDqLayer` is `None` (`svc_encode_slice.rs:712`). Today that is handled *inside* the
callees: fourteen bodies open with `if pCurLayer.is_null() { return .. }`. Flip the
parameter to `&SDqLayer` and the obligation silently moves to the call sites, which
become `&*current_layer(pEncCtx)` — **a null dereference the moment the layer is
unset**. It compiles, every test passes, and the defect lives on the path where
`iCurDqLayer` is `None`.

**The split the flip needs**, measured at this tree over 41 bodies taking
`*mut SDqLayer`:

* **4 writers — stay raw.** `UpdateMbListNeighborParallel`, `WelsSliceHeaderWrite`,
  `WelsSliceHeaderExtWrite`, `ReallocateSliceList`. (The brief says three; the fourth is
  `ReallocateSliceInThread`'s sibling `ReallocateSliceList`, and note that a
  writes-detecting *regex* misses `..sSliceBufferInfo[i].iMaxSliceNum = ..` because of
  the field path after the index — the compiler is the only reliable classifier here,
  which is the brief's own rule.)
* **10 readers whose null guard is load-bearing — need `Option<&SDqLayer>`**, with the
  guard rewritten as `let Some(x) = x else { <the guard's own return> };`:
  `slice_in_layer`, `WelsMbToSliceIdc`, `UpdateMbNeighbor`,
  `UpdateMbNeighbourInfoForNextSlice`, `WelsSliceHeaderScalExtInit`,
  `WelsSliceHeaderExtInit`, `OutputPMbWithoutConstructCsRsNoCopy`,
  `WelsGetNextMbOfSlice`, `SetSliceBoundaryInfo`, `WelsMdI16x16`. Four of these guard on
  a *compound* condition (`pCurLayer.is_null() || kiSliceIdx < 0`,
  `pEncCtx.is_null() || pCurLayer.is_null()`, …) and must be split by hand, not by
  pattern.
* **27 readers with no guard — take `&SDqLayer` directly.** Their callers already
  guarantee non-null, because those bodies already dereference unconditionally; forming
  `&*ptr` at the call site assumes exactly what the existing deref assumed, and no more.

**Cost, measured**: 125 call sites take `&*`, driven mechanically off rustc's spans;
that part converged in one pass, 296 errors to 39.

**Why it was reverted rather than finished.** The remaining work is ten hand edits, and
the attempt reached them through several broad regex passes over 150 sites. A
pattern-rewrite of `if x.is_null()` cannot tell which `x` is the layer, and applying one
to a half-converted tree — in fork-shared code, at the end of a long session — is the
shape that lands a silent defect. The prerequisites are committed and probe-verified;
the flip is mechanical from this finding and should be started from a clean tree with
the three groups above applied deliberately, writers first.

## F237 — the layer flip's own census was 4 short and 4 long: 37 bodies, not 41, and three more writers than the split named

F236 measured "41 bodies taking `*mut SDqLayer`" and split them 4 writers / 10
`Option` / 27 straight. Re-measured at this tree before applying the flip, the
42 `: *mut SDqLayer` sites are **not** 41 bodies plus one:

* **4 are function-pointer typedefs**, not bodies — `PDeblockingFilterSlice`
  (`deblocking.rs:275`), `PMdBackgroundInfoUpdateFunc` (`wels_func_ptr_def.rs:121`),
  `PWelsSliceHeaderWriteFunc` (`svc_encode_slice.rs:1373`), `PUpdateFMESwitch`
  (`svc_motion_estimate.rs:409`).
* **1 is a test local** — `let p_layer: *mut SDqLayer = &mut layer;`
  (`svc_encode_slice.rs:5170`).
* **37 are bodies**: 14 guarded, 23 unguarded. The guarded 14 are exactly F236's 4
  writers + 10 `Option` group, so those two groups were right; the straight group is
  **23, not 27**, and the four missing were the typedefs counted as bodies.

The typedefs matter beyond arithmetic: eleven of the 23 unguarded bodies are
`unsafe extern "C" fn` targets of those four typedefs, so a body cannot move without
its typedef moving in the same edit. F236 does not mention them at all.

**And the writer list was three short.** Applying the flip and letting rustc classify —
F236's own rule, "ask the compiler, not a regex" — turned up three more bodies that
cannot take `&SDqLayer`:

* `ReallocateSliceInThread` calls the writer `ReallocateSliceList`, so it is a writer by
  transitivity. The compiler says so at the call, not at any assignment, which is why
  no writes-detector over its own body would ever have found it.
* `DeblockingFilterSliceAvcbase` forms `&mut pCurDq.sMbDataP` for the whole-grid window.
  That is legitimate — its own comment records F108's verified claim that this is the
  *single-threaded* frame filter — but it is a `&mut` through the layer all the same.
  Its typedef `PDeblockingFilterSlice` and its sibling `DeblockingFilterSliceAvcbaseNull`
  stay raw with it.

The two `_c` forwarders `WelsSliceHeaderWrite_c` / `WelsSliceHeaderExtWrite_c` are
raw for the same transitive reason, and F236's split does not place them either.
So the true split at this tree, **measured after the flip landed, not derived**:
**9 stay raw / 11 `Option<&SDqLayer>` / 17 `&SDqLayer`** — with `UpdateFMESwitchNull`
moved into the `Option` group by F238 below.

**What did drive mechanically, exactly as F236 promised**: the call sites. 323 primary
error spans, auto-rewritten off rustc's own byte offsets by rule — `&*x` for
`expected &SDqLayer`, `x.as_ref()` for `expected Option<&SDqLayer>` from a raw, `Some(x)`
from a reference — converged in two passes to 4 errors, all four of which were the real
findings above. 124 derefs inserted; an audit of every one against a null guard within
30 lines flagged 8, of which 6 were `current_layer` sites whose callees deref
unconditionally (unchanged hazard) and 2 sat *after* `ReallocateSliceList`'s own guard.

## F238 — the straight group's rule has an exception it does not state: a body that never dereferences its layer is not safe to convert to `&`

F236 justifies the unguarded group with: "They already dereference unconditionally, so
forming `&*ptr` at their call sites assumes exactly what the existing dereference
assumed, and no more." That is true of 22 of the 23 — and false of the one body that
dereferences *nothing*.

`UpdateFMESwitchNull` is `pub unsafe extern "C" fn UpdateFMESwitchNull(_pCurLayer) {}` —
an empty body behind `PUpdateFMESwitch`, and `pfUpdateFMESwitch` is set unconditionally
to it (`svc_motion_estimate.rs:547`; nothing else is ever assigned). Its only call is
`encoder_ext.rs:4077`, `f(current_layer(pCtx))`. Passing a **null** raw pointer to a
function that ignores it is completely defined. Rewriting that site to
`f(&*(current_layer(pCtx)))` makes it **undefined the moment the layer is unset** — a
regression manufactured by the conversion, in the one body whose emptiness put it
outside the rule's premise.

The rule's real form: a body may take `&SDqLayer` when it dereferences the layer
unconditionally. "No null guard" is a proxy for that, and the proxy fails for no-ops.
A mechanical check catches the class — for every converted body, count uses of the
parameter name in the body; zero uses means the premise does not hold. At this tree
`UpdateFMESwitchNull` was the only one.

**The fix is better than reverting to raw**: `PUpdateFMESwitch` and
`UpdateFMESwitchNull` take `Option<&SDqLayer>`, and the call site becomes
`f(current_layer(pCtx).as_ref())`. Null stays representable, exactly as the raw pointer
made it, and the raw pointer is still gone. `Option<&T>` is niche-optimised to one
pointer and is FFI-safe, so the `extern "C"` typedef is unaffected.

Two further stale claims this checkpoint corrected, both in `svc_encode_slice.rs`:
`layer_ref_pic`'s doc asserted "the returned borrow is not tied to `pLayer` (the layer is
reached raw, so it cannot be)" — tying it is precisely what the flip does — and four
`# Safety` clauses required a "live layer" from parameters that are now references, which
is F231's class one file at a time.

## F239 — the layer flip's precondition was necessary but not sufficient: `&SDqLayer` is a whole-struct retag that pops field-precise raw derivations *on one thread*, and only Miri sees it

S5 cleared the flip's stated obstacle — "zero fields of `SDqLayer` are written from
inside the fork any more", measured, and F236 treated that as the go-ahead. It is a
statement about **concurrency**. The defect the flip actually shipped is about
**aliasing on a single thread**, and nothing in the plan anticipated it.

Miri, on `encode_loop_runs_over_size_limited_dynamic_slices_under_the_aliasing_checker`:

```
error: Undefined Behavior: trying to retag from <2973712> for SharedReadWrite
       permission at alloc309593[0x208], but that tag does not exist in the borrow stack
  --> src/encoder/encoder_ext.rs:2243:27   (write_bytes through pNalHdExt)
help: <2973712> was created by a Unique retag at offsets [0x208..0x220]
  --> encoder_ext.rs:2190   let pNalHdExt = &mut (*pCurDq).sLayerInfo.sNalHeaderExt;
help: <2973712> was later invalidated at offsets [0x0..0x2f8] by a SharedReadOnly retag
  --> encoder_ext.rs:2236   slice_in_layer(pCurDq.as_ref(), iIdx)
```

Read the two offset ranges: the `&mut` claims **[0x208..0x220]**, 24 bytes of one field;
`pCurDq.as_ref()` claims **[0x0..0x2f8]**, the whole 760-byte layer. Converting
`slice_in_layer` to `Option<&SDqLayer>` turned every call site holding a raw layer into
a whole-struct claim, and a whole-struct claim pops every field-precise derivation live
across it. The callee reads one `Vec` header. The retag does not care.

**Both differential sweeps passed this, twice, in both profiles — 583/583 each.** The
byte gate cannot see a tag it never materialises. This is the third defect the Miri lane
has caught behind a green sweep, and the first where the sweep ran *after* the aliasing
error was already in the tree.

**The shape, so it can be searched for and not rediscovered.** Within one body:

1. `let X = &mut (*L).field;` or `addr_of_mut!((*L).field)` — a Unique or
   SharedReadWrite claim on part of the layer;
2. any later call taking the converted layer — `L.as_ref()`, `&*L` — a SharedReadOnly
   claim on all of it, which pops X;
3. any later *use* of X, read or write.

A scan for exactly that three-step span over every layer-named binding found **5 spans in
4 functions**: `WelsInitCurrentLayer` (the one Miri reached), `WelsISliceMdEncDynamic`
and `WelsMdInterMbLoopOverDynamicSlice` (`addr_of_mut!((*pCurLayer).sSliceEncCtx)` held
across three retags apiece), and `WelsCodeOneSlice`
(`addr_of_mut!((*pCurLayer).sLayerInfo.sNalHeaderExt)` across `WelsSliceHeaderExtInit`).
**Miri reported one of the five** — it aborts at the first UB, so a green re-run after one
fix proves only that one fix. The scan is what covers the rest; the checker confirms it.

**The fix is live-range narrowing, not re-plumbing**: derive at the use. Each of the four
derivations had its uses either entirely before the retag or in one place after it, so
each became a fresh sibling derived from the still-live raw parent. `addr_of!` bindings
need no such care — a SharedReadOnly sibling coexists with a SharedReadOnly parent claim,
which is why the read-only derivation at `svc_encode_slice.rs:1581` is untouched.

**The rule this leaves for the context flip**, which is the same conversion one level up
and roughly 114 allows wide: `&sWelsEncCtx` will do this to *every* field-precise context
derivation held across it, and there are far more of those than there were layer ones.
Narrowing live ranges is a per-site fix that does not scale to that. Before the context
flip, the derivations that must survive it should move to shared (`&`/atomic/`Cell`)
access, where sibling claims coexist by construction — the tabulation the brief already
asks for should record, per field, whether its derivation is exclusive, because that is
the column that predicts this defect.

## F240 — the layer flip's cascade is 7 allows, not a wave, and `ref_list_mgr_svc.rs` is blocked by the *context*, not by `current_layer`

F232 and the S6 brief both name `ref_list_mgr_svc.rs` as the layer flip's big payoff —
"31 allows, *every one* on a signature with no raw pointer — blocked purely by body calls
into raw accessors, 17 of them `current_layer`", with the instruction to "rerun the
de-unsafe cascade and expect it to travel far". Measured after the flip landed:

The converging cascade — strip `unsafe` only from `unsafe fn` declarations whose
signature carries no raw pointer, compile, revert what rustc rejects, to a fixpoint —
offered **130 candidates and kept 7**, all seven of them bodies this checkpoint had just
converted (`WelsMbToSliceIdc`, `UpdateMbNeighbor`, `WelsGetNextMbOfSlice`,
`SetSliceBoundaryInfo`, `layer_rec_view`, `WelsRecPskip`, `UpdateFMESwitchNull`). All
seven then had empty-of-`unsafe` bodies, so their `#[allow(unsafe_code)]` and
`// unsafe-cat:` tags retired with them: **460 → 453**.

**Why the file did not move.** Stripping all 25 eligible declarations in
`ref_list_mgr_svc.rs` and classifying every resulting `E0133` by what it names:

| blocker | count |
|---|---:|
| raw pointer dereference (in-body, no callee) | 52 |
| `current_layer` | 17 |
| `ctx_param_raw` | 8 |
| `ctx_vpp_raw` | 5 |
| `slice_in_layer` | 4 |
| `CWelsPreProcess::{UpdateBlockIdcForScreen, GetRefFrameInfo}` | 2 |

`current_layer` is the largest *named* blocker, which is the true half of the brief's
claim. But it is 17 of 88, and the 52 bare raw dereferences plus 13 raw *context*
accessors are untouched by anything the layer flip could do — `current_layer` itself is
`unsafe fn` only because it takes `*mut sWelsEncCtx`, so converting the layer it returns
cannot make it safe. This file unlocks with the **context**, not the layer, and the plan
should carry it under that item rather than as a layer-flip dividend.

**The cheap piece that would move the 17**: a safe companion
`current_layer_ref(ctx: &sWelsEncCtx) -> Option<&SDqLayer>`, which is expressible with no
`unsafe` at all (`ctx.iCurDqLayer?` then `ctx.ppDqLayerList.get(idx)?.as_deref()`) because
every field it touches is already owned and safe. It is not enough on its own — the 52
raw dereferences still stand — so it belongs to the context checkpoint's tabulation, not
to a cascade rerun.

## F241 — the screen-content storage converts to owned buffers, and the two pointer tables had to become *index* tables; three things the plan's ruling did not mention

D-scope-5's reversal ruled the shape — "full ownership", with the two pointer tables as
index tables — and that ruling held. Three details it did not carry, each of which the
conversion had to settle:

**1. The arena has to travel with the index table.** `pLocationOfFeature` held per-value
*addresses* into `pLocationPointer`; as offsets they are meaningless without the buffer
they index. So `SFeatureSearchIn` grew a third field, `pLocationPointer`, that the
pointer spelling never needed, and `PFillQpelLocationByFeatureValueFunc` grew a
parameter for the same reason. Conversely `PInitializeHashforFeatureFunc` *lost* one:
its `pBuf` was the arena base every entry was measured from, and with offsets that base
is the constant 0.

**2. The two tables have different lengths, and one is not `iActualListSize`.** The C++
allocator sizes `pTimesOfFeatureValue` and `pLocationOfFeature` at `kiListSize` but
`pFeatureValuePointerList` at `WELS_MAX (LIST_SIZE_SUM_16x16, LIST_SIZE_MSE_16x16)` —
65281 regardless of the mode, where `kiListSize` is 16321 for 8x8 or 256 for a non-zero
strategy index. A conversion that read "three tables of `iActualListSize`" out of the
struct's shape would have silently shrunk the cursor table by 4x in 8x8 mode.
`SScreenBlockFeatureStorage::for_frame` reproduces all four lengths from
`svc_motion_estimate.cpp:690-721`.

**3. `#[derive(Default)]` is not the old `Default`.** The hand-written impl filled
`uiSadCostThreshold` with `UINT_MAX`; a derived one zeroes it, and a zero threshold is
the opposite instruction to the search — every candidate immediately "under" it. The
impl stays hand-written for that one field.

**The referee, since no gate reaches this code.** `AllocPicture` refuses
`iNeedFeatureStorage != 0`, nothing fills `SPicture::pScreenBlockFeatureStorage` (F229),
and `PerformFMEPreprocess` has no call site — so both sweeps are silent on every line
of this family, in both profiles, and always were.
`feature_storage_arena_invariants_hold_over_a_synthetic_frame` is the family's first
test: it builds the storage by hand (which owning the buffers is what makes possible),
runs `CalculateFeatureOfBlock` over a deliberately non-flat 48x32 luma, and asserts the
three arena invariants — histogram sums to the block count; the groups tile the arena
exactly, each cursor ending `2 * times[value]` past its base; every written position is
a whole-pixel qpel coordinate inside the frame. **Three controls, each seen red**:
advancing the hash by `times` instead of `times << 1` ("group 3393 does not start where
3392 ended"), advancing the fill cursor by 1 instead of 2 ("group 3392's cursor did not
end 2*1 past its base"), and biasing a written x ("x 1000 outside 40 block columns").
Runtime is under 0.01 s.

**Two smaller things worth the ledger.** `&mut [T]` in an `extern "C"` signature is not
FFI-safe (`improper_ctypes_definitions`), so `PCalculateBlockFeatureOfFrame` and its two
kernels drop `extern "C"` and stay plain `unsafe fn` pointers — the slot is internal
dispatch and its sibling `PCalculateSingleBlockFeature` was already a plain `fn`. And a
"dead `#[allow(unsafe_code)]`" detector that looks for the token `unsafe` in the body
misses `#[unsafe(no_mangle)]` on the *item*: it reported `WelsGetCodecVersion` as dead,
and removing the allow broke the build. The attribute is the unsafe there.

## F242 — the 444 remaining allows, re-classified at S6's close, and the two things the next session should know before it starts

F232 classified the then-472 allows and the S6 brief built its "no-seam middle" table
from that. Re-measured at 444, by what actually blocks each one (the classifier reads
the whole signature, not the first line — F232's own warning about a classifier that
counted `&mut sWelsEncCtx` as a raw pointer):

| blocker | count |
|---|---:|
| no raw parameter at all — body-blocked | 215 |
| `*mut sWelsEncCtx` | 133 |
| some *other* raw parameter | 87 |
| `*mut SDqLayer` | 10 |

The layer column is down to 10 and those are the nine bodies S6.A1 left raw plus a
typedef, so the layer is finished as a blocking edge. **The 87 "other" is the tractable
middle**, and it is not distributed the way the brief's per-file table suggests. By
pointee type:

| type | count | note |
|---|---:|---|
| `u8` | 53 | plane roots — the pixel family, a job of its own |
| `SLogContext` | 16 | see below |
| `i16` | 10 | coefficient blocks |
| `SWelsSvcCodingParam` | 9 | |
| `i32`, `SSliceArgument` | 7 each | `SSliceArgument` landed as S6.D1 |
| `BsWriter` | 6 | D1's remainder |

**The `SLogContext` family, analysed but deliberately not started.** 18 raw parameters
funnel into one sink, `WelsLog(pLogCtx: *mut SLogContext, ..)`, which is *already a safe
`pub fn`*: it null-checks, copies the struct out (`SLogContext` is `Copy`), and calls the
application's `pfLog`. The obvious conversion — `Option<&SLogContext>` — is **wrong for a
reason the function's own comment already records**: it copies the struct out precisely
so that "the callback's own re-entry (a callback that calls back into the codec)" does
not alias a live borrow. A `&SLogContext` parameter puts that borrow back, live across
the callback, in all 63 calling frames — F239's shape, one level out, and with a
re-entrancy path that Miri's encoder shards do not exercise because no test installs a
tracing callback that re-enters.

The shape that avoids it is **by value**: `SLogContext` is `Copy` and four words, the
sink already works on a copy, and a by-value parameter holds no borrow for a re-entrant
callback to collide with. Null then has to be spelled — and it already has a spelling,
because `SLogContext::default()` has `pfLog: None` and `WelsLog` returns early on that,
so a null pointer and a default-constructed context are observationally identical today.
That is the conversion to do; it is 18 parameters and 63 call sites and it wants a fresh
session, not the tail of one.

**And a note on running the tools.** The brief warns that `rust/tools/*.py` globbed from
the repo root match nothing and print a false zero. `safeplan_prohibitions.py` did
exactly that mid-session — "0 *mut-ctx bodies scanned against 0 writers", which reads
like a clean result rather than a non-run. From the crate root the same command reports
"106 ... no violations". A checker whose failure mode is a green-looking zero should say
so; until it does, every quoted prohibition result should carry its body count.

## F243 — the context flip needs no storage migration: across all 106 raw-context bodies the compiler finds exactly one write, in a single-threaded initialiser

The plan and the S6 brief both specify the `&sWelsEncCtx` flip as a storage campaign:
"tabulate every fork-reachable body by what it writes through the context, move each
worker-written field to atomics / `Cell`s / separately-allocated storage (each move with
its own targeted two-thread probe, control seen red), and only then flip bodies". That is
what the layer took across S5 (two field moves, two probes) and S6 (the flip). It is
budgeted as "likely a whole session by itself".

Measured, it is not that job at all. Flipping all 106 parameters and 9 typedefs to
`&sWelsEncCtx`, rewriting the call sites off rustc's spans, and clearing residual type
errors to **zero** — the condition for MIR borrowck to run over every body — leaves **five
errors, total**:

* **one `E0596`**, `pEncCtx.sWelsCabacContexts` in `WelsCabacInit`
  (`set_mb_syn_cabac.rs:830`) — the only direct write through the context anywhere. It has
  one caller, in `WelsInitEncoderExt`, and is not fork-reachable; rustc's own suggestion is
  `&mut sWelsEncCtx`.
* **four `E0502`**, all `ParasetStrategy(pCtx)` holding a whole-context borrow across a
  disjoint `pCtx.pOut` mutation, all in single-threaded parameter-set code, and
  `ParasetStrategy` is in the fork-split tool's ST-FLIPPABLE list.

**Zero writes from inside the fork.** That does not mean workers write no context state —
it means every field they write already reaches them through interior mutability or a
separate allocation, which earlier sessions did incrementally and which
`safeplan_prohibitions.py` has been reporting as "0 writers" all along. What is new is that
the compiler now confirms it across all 106 bodies simultaneously, with types clean, rather
than one prohibition scan at a time.

**So the flip's real cost is three structural items, none of them storage**: (a) it must go
root-down, because a body cannot take `&sWelsEncCtx` while it still passes the context to a
raw-taking callee — flipping one body in isolation yields only `E0308` at its onward calls
and never reaches borrowck, which is also exactly why `ref_list_mgr_svc.rs` did not move in
S6 (F240); (b) `SliceJobHandle` stores the context across `thread::scope` under
`unsafe impl Send`, so the field wants `&'a sWelsEncCtx` plus a struct lifetime and
`sWelsEncCtx: Sync` — the design work, and its own checkpoint; (c) four whole-context
accessor borrows to split into disjoint fields, the fix S6 already used for
`DeblockingFilterFrameAvcbase`.

**And the F239 column is empty, with its control seen red.** A scan for the layer flip's
defect shape at the context level — raw parent, field-precise exclusive derivation, later
whole-context call, later use — finds nothing; injecting the pattern into `WriteSliceBs`
makes it fire at `slice_multi_threading.rs:941`. There is a structural reason to expect
that to hold: once a parameter is `&sWelsEncCtx`, `&mut (*pCtx).f` inside it is a compile
error rather than silent UB, and the transition sites where a *caller* still holds the
context are the four `E0502`s above — which rustc catches precisely because those callers
hold a reference and not a raw pointer. The layer flip's hazard came from raw parents,
which borrowck does not track. `SliceJobHandle` is the one raw-context-across-a-call object
left, and it is named rather than diffuse.

**A measurement correction this turned up.** `106` is right and the S6 close's `133` figure
counts allows, not bodies — but a grep for `: *mut sWelsEncCtx` finds only **102**, missing
four parameters written `: *mut crate::encoder::encoder_context::sWelsEncCtx` in
`set_mb_syn_cabac.rs` and `svc_set_mb_syn_cabac.rs`. Those two files do not appear at all in
a per-file survey built on that grep. `phase9_forksplit.py` had 106 all along; the grep was
short, and F242's per-file table inherits the blind spot.

The full tabulation, with the body inventory, the ordered blocker list and the caveats on
the measurement, is `rust/docs/phase9_context_flip_tabulation.md`.

## F244 — `ParasetStrategy` needed no raw pointer and no detach: the self-referential borrow dissolves because every method's reach into the context was one accessor call

The S7 tabulation named `ParasetStrategy` as design work of the same kind as the
`SliceJobHandle` seam: the strategy object *lives inside* the context
(`pFuncList.pParametersetStrategy`), its methods took the context again, so
`ParasetStrategy(pCtx).UpdatePpsList(pCtx)` is two `&mut` claims on one allocation and
was expressible only through a raw pointer. The expected fixes were a `take()`/restore
window or a long narrowing chain.

Neither was needed. **Every context-taking method's reach into the context was one or two
accessor calls**, measured by reading each body:

| method | what it actually touched |
|---|---|
| `UpdatePpsList` | `iPpsNum`, the PPS array |
| `UpdateParaSetNum` | `iSpsNum`, `iSubsetSpsNum`, `iPpsNum` |
| `InitPps` | `pps_array()` / `pps_array_mut()` |
| `GenerateNewSps` | `param_and_paraset_arrays_mut()` — and, transitively, `WelsGenerateNewSps` (the same accessor) and `SpsReset` (`sps_array_mut`, `subset_array_mut`) |
| `OutputCurrentStructure` | `sps_array()`, `subset_array()`, `pps_array()` — all shared |

So each method takes those values instead, and each call site splits them off one
`&mut sWelsEncCtx` alongside the strategy — five helpers in the shape
`encoder_context.rs:1028`'s `ctx_paraset_arrays` already established, whose own comment
states the principle exactly: *"Rust permits `(&mut s.a, &mut s.b, &mut s.c)` from one
`&mut s` inside a single body … What it cannot see is three accessor calls each claiming
the whole context."* The self-referential borrow was never intrinsic; it was two accessor
calls where one split would do. `ParasetStrategy` is now a safe
`fn(&mut sWelsEncCtx) -> &mut CWelsParametersetIdStrategyObj`, and
`paraset_strategy.rs` holds **zero** raw context parameters.

**Three prose comments in the tree were arguing borrowck's case by hand, and all three
are now type-level.** `encoder_ext.rs:912` — *"Hoisting is sound as well as legal — the
strategy object lives in the `pFuncList` `Box`, a different allocation, so the reborrow
below cannot pop it"*; the S67/H2 note at `wels_encoder_ext.rs:925` making the same
argument for `OutputCurrentStructure`; and `encoder_ext.rs:982`'s F208 note about a read
*"while it is live … and the reason it was invisible before is that `ctx` was a raw"*.
Each was true, and each needed a raw pointer to express. The third dissolved by
**ordering** rather than splitting — the `bool` is read before the strategy borrow opens.

**A measurement error worth recording, because it is the second time this session.**
Grepping a method body for `pCtx\.[A-Za-z_]+` to find "what it uses from the context"
misses `pCtx` passed *as an argument* — `WelsGenerateNewSps(pCtx, ..)` and
`self.SpsReset(pCtx, ..)` were both invisible to it, and `GenerateNewSps` looked like a
one-accessor body when it was a three-call chain. The chain turned out shallow, so the
conclusion held; it did not have to. Pair any "what does this touch" grep with `\bpCtx\b`
unqualified. (F243 records the sibling error: `: *mut sWelsEncCtx` misses
fully-qualified-path parameters.)

Also corrected here: the tabulation's §2 attributed four `E0502`s to `ParasetStrategy`.
Three of the four were at *different* sites than the flip experiment suggested — the
experiment's lifetime pass had moved where the conflict surfaced — and converting the
function produced nine conflicts, not four, across five distinct shapes. The count was
never the point, but the tabulation should not be read as an exhaustive site list.

## F245 — the `SliceJobHandle` checkpoint dissolves: `sWelsEncCtx: Sync` is unreachable *and* the flip never needed it

S7's tabulation named the fork seam as "the design work of the flip" and said making
`SliceJobHandle::pCtx` a `&'a sWelsEncCtx` "requires `sWelsEncCtx: Sync` — which is the
same audited-`unsafe impl` shape the reconstruction view already uses, not a new kind of
claim". That sentence is wrong twice, and the first half was already answered in the tree
by a comment I had read past.

**1 — `Sync` is structurally unreachable.** The `unsafe impl Send for SliceJobHandle`
carries a long audit whose F205 section derives the context's `!Sync` reasons *by field*
and concludes: `SLogContext` holds the application's opaque `*mut c_void`, installed
through `SetOption(TRACE_CALLBACK)`, so "no amount of internal conversion can make a
caller-owned `void*` `Sync`… It stays, per **D-exit-2**." A ruled decision, and the
tabulation contradicted it without citing it.

**Re-derived at S7** — one `Sync` bound on the context, read the compiler's chains — the
reasons are **six root types under three fields**, against F205's twelve types under
seven:

| field | roots | owner |
|---|---|---|
| `ppDqLayerList` | `*mut SRefList`, `*mut SrcPicPool` (through `SDqLayer`) | the layer family |
| `pVaa` | `*mut u8`, `*mut i8`, `*mut u32` (through `SVAAFrameInfo`) | the VAA family |
| `sLogCtx` | `*mut c_void` | **the C ABI — irreducible** |

Four of F205's seven retired: `pSliceThreading`, `pOut`, `pVpp` to earlier sessions, and
`ppRefPicListExt` to **S6.B1**, which made `SPicture::pScreenBlockFeatureStorage` own its
buffers. Converting the other two families would leave `sLogCtx` standing alone, so `Sync`
does not arrive even then.

**2 — and none of it is needed, which is the part that matters.** The flip does not send a
`&sWelsEncCtx` across the spawn. It sends the *handle* — a raw pointer, which is precisely
what the `Send` impl covers — and each worker forms `&*job.pCtx` **after arrival, on its
own thread**. No `Sync` bound is demanded for that. Verified rather than reasoned: a
`fn(&sWelsEncCtx)` called from inside the spawned closure with `&*job.pCtx` compiles
unchanged, and S7.A1's whole-tree experiment had already wrapped those three sites the
same way and reached zero type errors.

What the worker-local `&sWelsEncCtx` *does* require is the three-part disjointness
argument the `Send` impl already carries — index-based slot ownership, order-based
assembly, and the buffer-count cap. The flip widens who relies on that argument without
changing what it claims. **So the handle keeps its raw field and its `unsafe impl`, which
is the plan's stated end state (§7.4's two audited lines) and not a debt**, and the flip's
prerequisite list is one item shorter than the tabulation said.

**The lesson for the tabulation as an instrument.** Its §2 (the write set) was measured and
held. Its §4 (the blocker list) mixed measurement with inference, and both inferred items
were wrong: §4(c) called `ParasetStrategy` mechanical when it was not (F244), and §4(b)
called this a blocker when it is not. Where the compiler was asked, the tabulation was
right; where it was reasoned about, it was not. A blocker list should carry, per item,
which of the two it is.

## F246 — the context flip lands: 106 raw-context bodies to zero, and the tabulation's residue counts were inflated by comments

The flip the plan budgeted a whole session for, with a storage campaign in front of it,
landed as one mechanical checkpoint on top of S7.A2–A4. Measured before and after:

| | before | after |
|---|---:|---:|
| bodies taking a raw context | 106 | **0** |
| `safeplan_prohibitions.py` bodies scanned | 106 | **5** |
| raw-context *sites* in the tree | 115 | **6** |
| allows (outside `src/api/`) | 440 | **431** |

The six survivors are not residue: five are the `ctx_*_raw` slot readers F71 designed —
they read a pointer out of a `Box` slot without forming a reference, which is the whole
point — and the sixth is `SliceJobHandle::pCtx`, which F245 established stays. All six are
`*const`.

**The cascade is where the flip pays.** Its candidate pool — declarations whose signature
carries no raw pointer — went **129 → 209** the moment the flip landed, because 80
signatures stopped being raw. Eleven stripped, nine allows retired behind them, in files
the flip never touched by hand (`rc.rs` four, `svc_encode_slice.rs` two,
`svc_mode_decision.rs` two, `svc_encode_mb.rs` one).

**Two of the tabulation's residue counts were wrong, both inflated by comments.** It said
"56 context `is_null()` call sites". There are **eleven**, in `src/encoder/`; the other 45
matches are *comment text* — mostly the T9.H notes recording that an earlier session had
already retired one ("the `pCtx.is_null()` disjunct is gone"). The S6 brief warns that the
prohibition checker counts comments; the same trap catches any `grep -c` over a codebase
that documents its own history this thoroughly. Five of the eleven were simple, three were
compound and had live arms that had to be kept by hand (a layer null, a `pSliceCtx` null,
and two dimension tests), and two were five-arm guards where only the first arm retired.

The "10 re-casts" was closer but also comment-inflated; the real collapses were four
`&*(std::ptr::addr_of_mut!(*ctx))` round-trips — a reference laundered through a raw and
back, which the flip makes visibly pointless — plus the qualified-path forms.

**The null-guard obligation, discharged by enumeration rather than by the general claim.**
The tabulation's own caveat required confirming each guard's callers rather than deleting
on "a reference cannot be null". Every root that mints a context pointer was enumerated:
the three fork entry points (`EncodeFixedSlicesForked`, `UpdateMbMapForked`,
`EncodeSizeLimitedSlicesForked`) all take `&mut sWelsEncCtx`; every other root is
`&mut *ctx` or `addr_of_mut!(*ctx)` off an owned `Box`; and no body among the 106 is
reached from `src/api/`. No null can arrive, so the guards were dead — the same conclusion
T9.H reached for the bodies it converted, now checked at the roots instead of assumed.

**F238's exception fired again, and rustc caught it again.** `WelsRcMbInfoUpdateDisable`
is an empty body behind the `pfRc.WelsRcMbInfoUpdate` dispatch, and a test passed it
`&*(std::ptr::null_mut())` — undefined the moment the parameter became a reference, where
handing a null raw pointer to a body that ignores it was fine. Unlike S6's
`UpdateFMESwitchNull`, the fix here is the *test*, not the signature: the production
dispatch's context comes from the encode path and its sibling arm dereferences
unconditionally, so `&sWelsEncCtx` is right and the null was a test artifact. The stale
comment beside it — "keeps its raw context and its null … its parameter is staying `*mut`"
— is corrected in place.

**And `SliceJobHandle::pCtx` narrowed to `*const`.** The three sites that read it form
`&*job.pCtx`, and S7.A1 measured the context's write set at one body which is not this
one, so nothing writes through the handle. `*const` states that; the `unsafe impl Send`
is unchanged and still necessary, because raw pointers are `!Send` whatever their
mutability.

## F247 — the F239 span scanner, rebuilt: the clean tree did not report zero, and the extra hit was the instrument's fault

S8's brief made the span scanner step 0 and named its acceptance exactly: "the clean
tree reports **zero**, and the recorded control — injecting the pattern into
`WriteSliceBs` — reports exactly one hit (`slice_multi_threading.rs:941`)". Both
halves needed correcting, in opposite directions.

**The control's line number has moved.** The derivation is at
`slice_multi_threading.rs:945` now; the file gained four lines after S7 recorded 941.
Injecting a whole-struct reborrow between that `addr_of_mut!` and its first use fires
once, at the derivation, as recorded.

**A stronger control exists and the brief did not know it.** S6.A1 landed the layer
flip and its four narrowings in one commit, so the tree carrying F239's five live
spans was never committed and the `WriteSliceBs` injection is a synthetic stand-in.
But the narrowing is separable: `WelsInitCurrentLayer`'s own comment says the binding
"stood 50 lines up, next to `iSliceCount`", and moving it back reconstructs the real
defect. The scan reports it at derive@2209 / reborrow@2254 / use@2268 — Miri's
2190/2236/2243, renumbered. **That tree still `cargo check`s clean**, which is the
whole reason the instrument exists.

**The clean tree reported one span, not zero.** `WelsEncoderEncodeExt` derives
`pLayerBsInfo` from `(*pFbi).sLayerInfo` at 3496, `ClearFrameBsInfo(pCtx, &mut *pFbi)`
retags the whole struct at 4068, and `(*pLayerBsInfo).rPsnr[0]` writes at 4177: F239's
three steps in line order. It is not the defect — the reborrow's branch ends
`return ENC_RETURN_SUCCESS;`, so the use is unreachable from it. **Line order is not
execution order**, and a first-cut scanner cannot tell them apart.

F123's clause decides what to do about that: a scanner that fires on a sound spelling
trains the session to argue with it, and the next real hit gets the same argument. So
the fix went into the instrument, not into a waiver table. Three refinements, each
sound rather than convenient:

| refinement | why it is sound | what it would otherwise do |
|---|---|---|
| a reborrow whose block unconditionally exits shields only what is *outside* that block | control flow provably cannot reach | the `WelsEncoderEncodeExt` false positive |
| a derivation's live range ends when its own block closes | plain Rust scoping — the name past that point is a different binding | double-reports a nested re-derivation |
| a re-binding ends the range only at the derivation's own nesting level | a `let` in a nested block is scoped to it | silently drops real spans |

Calibration inherited rather than reinvented: `addr_of!`/`&(*R).f` are **not** flagged
(F123 — a read does not remove `SharedReadWrite` items above the tag it reaches
through, so the shared sibling survives), `addr_of_mut!` **is** (F239 recorded two such
spans; F208's contrary second-order note is an argument the instrument reports rather
than adjudicates). Statements are joined logically, because F208's line-based scan
misses `let pParamInternal =` with its `addr_of_mut!` on the next line — a real shape
in this tree.

Clean tree: **0 spans / 123 exclusive field derivations / 2633 bodies / 84 files.**
Selftest: 15 fixtures, both directions plus every calibration axis above.
`rust/tools/f239_span_scan.py`, and it runs from the crate root or exits 2 rather than
printing a green-looking zero.

## F248 — the de-unsafe cascade's expected reach was inferred twice and wrong twice; the blocking edge is the coding-parameter family

S8's brief predicts a cascade after each of its first two steps. Both predictions are
*inferred* rows in the brief's own sense, and both are wrong — measured after the fact:

| step | brief's prediction | measured |
|---|---|---|
| 1 (log context) | "`au_set.rs` (10 allows) and `paraset_strategy.rs` (7) should empty or nearly" | 10 and 7, **unchanged** |
| 2 (`BsWriter`) | "`nal_encap.rs` (9) and the two `svc_set_mb_syn_*` files (7 + 8) close or nearly" | 9→8, 7→5, 8→**8** |

**Why, from the compiler rather than from the shape.** Stripping `unsafe` from
`au_set.rs`'s four log-context declarations yields **62** E0133s, every one a raw
dereference of `*mut SSpatialLayerConfig` or `*mut SWelsSvcCodingParam` in the body;
`paraset_strategy.rs`'s `CheckParamCompatibility` is blocked by
`*mut SWelsSvcCodingParam` the same way. `svc_set_mb_syn_cabac.rs` has no writer
parameter at all and could not have moved for step 2 under any outcome.

The log context and the bit-writer were each *a* raw parameter on these signatures,
never the blocking one. **The blocking edge across all four files is the coding-
parameter family** — `SWelsSvcCodingParam` and `SSpatialLayerConfig` — which S8's
plan does not carry as a step at all. It should: it is the cheapest remaining named
edge, and two separate checkpoints have now been priced against it by accident.

The general rule this earns, beside the brief's own "label rows measured or
inferred": a signature with N raw parameters retires its allow when the **last** one
goes, so a conversion's cascade is predicted by the *residue* on each signature, not
by the parameter being converted. The cascade tool now makes that a measurement
rather than an estimate — `rust/tools/deunsafe_cascade.py --dry-run` lists exactly
the declarations whose signature is already clean.

## F249 — the pixel family is blocked on plane *roots*, which are storage and not parameters; steps 3, 4a and 5 are not executable in the brief's order

> **AMENDED the same day, and the amendment retracts half of this finding.** The
> ordering claim below stands: steps 3, 4a and 5 cannot convert parameters before
> the roots have views. The *diagnosis* did not — this finding called the roots "a
> storage-ownership question of the same kind F241 answered" and said it "needs the
> user's ruling". **Ownership was never the question, and the tree already said so
> in four places:**
>
> * `SPicture` **owns** its pixels — `[PaddedPlane; 3]`, owned since **T6.F2**.
>   `encoder/picture.rs:513`'s `pData: [*mut u8; 3]`, which this finding cited as a
>   root, is a *view* struct: its own doc says "copied out of it".
> * Picture **identity** already exists and is already used at the stamping site —
>   `pEncPic: Option<SrcPicId>`, `pDecPic: Option<RecPicId>`.
> * The **view-and-cursor pattern is in production**: `RecPicView` is a field on
>   `SDqLayer` (`pRecView`), resolved by `layer_rec_view()` at 28 sites, handing out
>   safe `RecCursor`s at 64. `PlaneCursor { buf, center, stride }` is the brief's
>   "biased-cursor form", already written.
> * **The hard half is done.** `RecPicView` solves the *concurrent write* seam —
>   workers writing one picture where no `&mut` can express the disjointness
>   (D-mt-3 option A). What remains is the **read-only** half, which is strictly
>   easier: `pEncData` is never written through (the `md.rs` hits are a local cursor
>   walk), and `SVAAFrameInfo`'s six planes are stamped from the same
>   `SPicture::planes()`. Both reuse `SharedPlane`/`SharedCells<u8>`, whose `Sync`
>   is **already one of the two audited `unsafe impl` lines** the end state budgets
>   for — so this adds no new one, no lifetime parameter, and no layout change
>   (`SDqLayer` lives in `Vec<Option<Box<SDqLayer>>>`, outside the pinned image).
>
> So the plan was missing a **checkpoint**, not a ruling. The lesson is the one this
> session kept relearning from the other side: this finding inferred a blocker's
> *kind* from the shape of the obstruction, having measured only that the roots were
> raw. Measuring what sat behind them — one `grep` for `struct SPicture` — would have
> found the storage already owned and the pattern already built. **F248's rule
> applies to blockers as well as to cascades: the residue is what must be measured,
> not the thing in front of it.**
>
> **RULED (user, 2026-08-30): one checkpoint, both views together.** An
> `EncPicView` for the layer's source planes and a VAA view for
> `SVAAFrameInfo`, built on `RecPicView`'s template in a single gated
> checkpoint (~99 sites: `mb_cursor` 18, `pEncData` 35, `pCsData` 10,
> VAA planes 36), then steps 3/4a/4b/5 run behind it. It gates the largest
> block left — **132 allows, 32% of the remaining 416, across 15 files**.

S8's steps 3, 4a and 5 convert pixel and coefficient *parameters* onto slices and
cursors. None of the three can start, and the reason is the same for all of them and
sits outside every file they name.

**Step 3's DCT half.** `PDctFunc`'s kernels (`WelsDctT4_c`, `WelsDctFourT4_c`) are
thin `extern "C"` shims that already reconstitute slices and call safe kernels
(`dct_4x4`). Their pixel arguments come from `mb_cursor`, which is
`encoder_context.rs:248`: `fn mb_cursor(&self, roots: &[*mut u8; 3], strides: &[i32; 3],
plane: usize) -> *mut u8`. The roots are `SDqLayer::pCsData` and `pEncData`,
`[*mut u8; 3]` each at `svc_encode_slice.rs:1098/1101`. Converting the parameter
requires the caller to hold a slice; the caller holds a raw derived from a raw field.

**Step 3's copy half.** `PCopyFunc`'s eight slots are called from
`svc_mode_decision.rs:547` as `copy16((*pVaaInfo).pCurY.offset(kiOffsetY), …)`.
`SVAAFrameInfo`'s six plane fields are `*mut u8` at `wels_preprocess.rs:499-504` —
**step 8's file**, four steps later.

**Step 5.** `common/mc.rs`'s three allows (`shim_wh`, `McLuma_c`, `McChroma_c`) and
`common/copy_mb.rs`'s one (`copy_shim<W,H>`) are the same plane-root kernels one
level down. `encoder/deblocking.rs`'s five are not convertible either, for unrelated
reasons: two are the ruled-raw in-fork layer writers, one is a tagged test
instrument, and the remaining two already take references and are body-blocked.

The measured root inventory, all struct **storage** fields:

| field | type | file |
|---|---|---|
| `SDqLayer::pCsData`, `pEncData` | `[*mut u8; 3]` ×2 | `svc_encode_slice.rs:1098,1101` |
| `pEncMb`, `pRefMb`, `pCsMb` | `[*mut u8; 3]` ×3 | `svc_mode_decision.rs:190-192` |
| `SPicture::pData` | `[*mut u8; 3]` | `encoder/picture.rs:513` |
| `SVAAFrameInfo` planes | `*mut u8` ×6 | `wels_preprocess.rs:499-504` |
| `pPixel`, `pVaaBlockStaticIdc`, … | `[*mut u8; 3]`, `[*mut u8; 16]`, … | `wels_preprocess.rs:268,600,601` |

**This is a blocker that needs the user's ruling, and it stops three checkpoints.**
The pixel family cannot be converted parameter-first: the roots are the work, and
they are a storage-ownership question of the same kind F241 answered for the
screen-content arena ("full ownership", index tables where pointers were stored).
Until a plane-root step exists and lands, steps 3, 4a and 5 convert nothing — a
kernel that takes `&mut [u8]` whose caller builds it with `from_raw_parts` has moved
the `unsafe`, not retired it. The two `extern "C"` typedefs would additionally have
to drop `extern "C"` first, on F241's precedent (`&mut [T]` is not FFI-safe, and
these slots are internal dispatch).

## F250 — the plan's single progress metric counted its own documentation: 431 was 424

`safeplan_tracking.sh` greps for `#[allow(unsafe_code)]` and filtered nothing, so a
**comment** mentioning the attribute scored as an allow. Seven did, outside
`src/api/`:

    opening commit f41fd200:  431 raw  ->  424 attributes   (api 45 -> 44)

**Six of the seven are comments warning that this exact count is wrong.**
`pic_queue.rs:31`, `decoder_core.rs:54`, `nalu.rs:12` and `decode_slice.rs:13` each
state that `src/decoder/` "carries **two** `#[allow(unsafe_code)]` items in total
(T8.A8)"; `pic_queue.rs:1088` states that it "reads **three by grep**". The tree
diagnosed the defect in prose and the tool then committed it — the warnings became
part of the number they warned about.

F219 built this command *because* the figure is the plan's single progress metric and
a hand correction had moved a right number to a wrong one. A command that counts
prose is that failure with a longer lifetime. F246 found the same inflation in the
context-flip tabulation's residue counts, and S8's brief carries the rule for
checkers — "the prohibition checker counts comments … denominators, always" — applied
to every instrument except the one that produces the headline.

Fixed: matches behind `//` or `//!` are excluded, and both figures print, so the
plan's existing tables stay readable against the basis that produced them. Every S8
figure below is the attribute count.

## F251 — the decoder residue was already complete: two mandated instruments, not eight convertible allows

S8's step 9 opens "The decoder's 8 remaining allows (`picture.rs` 3, `pic_queue.rs` 2,
`decoder_core.rs`/`nalu.rs`/`decode_slice.rs` 1 each)". Six of those eight are the
comment mentions of F250. `src/decoder/` carries **two** real attributes, both in
`picture.rs`'s test module, and both are **mandated instruments rather than tolerated
debt**: S28 requires a Miri test that reads `data_ptr`'s full legal reach in both
directions, and reading through a raw pointer is what that instrument *is*. The file's
own comment says so, and says they retire with `data_ptr` itself when family 13's
callers stop taking plane pointers — which is F249's plane-root work, not step 9's.

So step 9's decoder half needed no conversion and has none available. Its second half
— the trace newtype — did land (S8.9), but its stated purpose, "so `common/` seals
completely", is not reachable this session either: `common/mc.rs` (3) and
`common/copy_mb.rs` (1) are F249 plane-root kernels. `common/` goes 6 -> 4 and
`common/wels_trace.rs` seals on its own.

## F252 — the plane views land for the source planes, and **S9.0b is refused by a standing ruling**: the VAA "cur" planes are a fork-visible *write* target on an ungated path

> **SUPERSEDED THE SAME DAY BY F253 — and the way it was wrong is the point.** This
> finding refused the conversion on T9.B20's authority, quoting F117's "nothing in
> the gate battery can see it". **That premise had been false since session B4**,
> whose `bg` preset F126 calibrated with a planted fault in this very function's
> luma copy, failing all three clips. The sites converted in S9.0c.
>
> What survives is the *diagnosis*: `pCur*` is the destination, the write is in-fork
> into the picture the encoder reads, and a read-only view is the wrong type for it.
> That is why the conversion needed S9.0b's `SharedPlane`-backed view first. What
> does not survive is the conclusion — and it did not survive because a user
> disputed it, not because any instrument flagged it. **A deferral's premise expires,
> not just its conclusion**, and F126 sat 120 lines below F117 in this same file.

S9.0a converted the source planes and the DCT family behind them (commit
`08a2be27`): `RoPicView` through `&SPicture`, `pEncView` on `SDqLayer`, `PDctFunc`
off `extern "C"` and onto `(&mut [i16], &PlaneCursor, &PlaneCursor)`, both shims
safe, 13 call sites, Miri 296/0 and sweeps 583/583 in both profiles. Three things
that conversion settled are worth keeping:

* **The geometry proof came before the edit.** `mb_offset` is
  `((iMbX + iMbY * stride) << shift)`, which expands to exactly `x + y * stride` for
  `luma_origin`/`chroma_origin`'s `(x, y)`; the raw root is the plane's padded
  origin, the same anchor `RoPlane::cursor` measures from.
* **A trap that would have diverged on chroma only.** `mb_cursor` resolves *both*
  chroma planes through `strides[1]` (`stride_idx`), while a view's plane carries
  its own. They agree only because `AllocPicture` builds planes 1 and 2 with one
  `kuiChromaStride` and `iLineSize` is read back off those planes. Had a picture
  ever carried differing chroma strides, the conversion would have been silently
  wrong on U and V and byte-identical on Y.
* **The one new `unsafe` is intrinsic.** `RoPlane::buf` turns the stored base and
  length back into a shared slice, because a view that outlives the borrow which
  built it must store a base and a length and there is no safe way back. The
  alternative is a lifetime on `SDqLayer` propagating into `sWelsEncCtx` and the
  fork seam — F245's shape, where designs here die.

**S9.0b does not follow, and the reason is a ruling this session would have had to
override rather than a difficulty it could have worked through.**

The user ruled "one checkpoint, both views together" on a presentation — mine —
that both halves were the same kind of work. They are not, and the error was the
same one F249 made: **inferring a family's kind from the shape of its obstruction
instead of measuring the residue.** `SVAAFrameInfo`'s six plane fields *look* like
the source planes one level over. Measured:

1. `PCopyFunc` is `(pDst, iStrideD, pSrc, iStrideS)`, so at
   `svc_mode_decision.rs:549` **`pCurY` is the destination**. The VAA background
   path *writes* the current source picture; it does not read it. A read-only view
   is the wrong type for three of the six fields.
2. **F117 measured the destination to be the picture the encoder is reading** — a
   probe comparing `(*pCtx).pEncPic` against `GetCurrentOrigFrame(did)` reported
   `same:true` on **18 frames of 18** — and it happens in-fork, per macroblock.
3. **No gate can see it.** The diffharness sets
   `bEnableBackgroundDetection = false`, so the path is outside every sweep row in
   both profiles *and* outside the encoder-scoped Miri step. S9.0a's own green
   gates say nothing about it.
4. **T9.B20 already decided it**: the three copy sites stay raw and tagged, for the
   write-under-shared-view problem (D-mt-3) to take as a whole. Rule **S57** is the
   generalisation — *before converting a site, ask whether any gate runs it; if none
   does, that is an argument for leaving it raw, not for converting it carefully.*

So S9.0b would have converted the one aliasing shape in this family that is
genuinely hard, on the one path no gate exercises, against a standing decision.
Refused, and the ruled checkpoint closes at its first half.

**The point-of-use design was also not expressible as approved**, independently of
the above. `VaaBackgroundMbDataUpdate(pFunc, …, pVaaInfo: &SVAAFrameInfo, pCurMb)`
holds neither a pool nor a context, so a consumer cannot build a view at its own
use; point-of-use would first require `SVAAFrameInfo` to carry picture *identity*
(`SrcPicId`) rather than pointers — F241's index-table shape a third time — which
is a larger change than the phrase suggests and belongs with D-mt-3.

**What S9.0a inherits from F117, recorded on the type.** `RoPicView` hands out
shared slices over the source picture, and F117's dark path writes that same
picture through raw roots. The existing discipline covers it — *build a cursor at
its use and drop it in the same call*, `svc_mode_decision.rs:630` — and every
caller S9.0a converted satisfies it, since each cursor is an argument to one kernel
call. But it is now a constraint on a public API, on a path no gate walks, so it is
documented on `RoPicView` itself rather than left in the family's folklore.

## F253 — a deferral's *premise* expires, not just its conclusion: F117's three copy sites convert, because the gate it said did not exist has existed since session B4

T9.B20 left `VaaBackgroundMbDataUpdate`'s three copies raw, and stated its reason
plainly: *"nothing in the gate battery can see it"* — the diffharness pinned
`bEnableBackgroundDetection` false, so the path was outside every sweep row.
**Session B4 built exactly that gate** (D-ref-1), and F126 recorded its calibration
120 lines further down the same file: 48 rows, three clips, `t=4` chosen *because*
the function writes the source picture from inside a slice thread, entry probed at
thousands of `WelsMdBackgroundMbEnc` calls per row against a **dark control of
zero**, and the load-bearing line — *"a planted one-sample fault in
`VaaBackgroundMbDataUpdate`'s luma copy fails all three clips"*.

So the deferral's premise had been false for several sessions. S9's brief, this
session's own F252, and the report that accompanied it all repeated F117's sentence
without checking whether a later finding had overtaken it; **the user disputed it,
and the user was right.** The rule: *a decision's premise expires, not just its
conclusion. A deferral resting on a measurement must be re-read against the
measurements taken since — especially in a file that appends.*

**The conversion, and what it needed.** `PCopyFunc` drops `extern "C"` and its two
raw pointers for `fn(&RecCursor, &RecCursor)`. Both operands are cell-based, and
that is forced rather than stylistic: the slot's two callers disagree about storage
— the background path copies picture-to-picture, the mode-decision path copies an
owned prediction scratch into a picture plane — and **a function-pointer table
cannot be generic**. `RecCursor::over_owned` brings the scratch to the same type
through `Cell::from_mut(..).as_slice_of_cells()`, the standard library's own safe
door from an exclusive borrow to shared-mutable cells, so the unification costs no
`unsafe` at all.

`SVAAFrameInfo`'s six `*mut u8` plane roots become two `RoPicView`s. Storing a view
is correct *here* where a slice-based one would not be: the destination is written
in-fork into the picture the encoder is reading (F117's probe: `same:true` on 18
frames of 18), and cells make that lawful where a `&[u8]` over the plane would claim
every byte and race it.

The byte offsets become sample coordinates and name the same addresses:
`kiOffsetY = ((iMbY * stride + iMbX) << 4)` expands to
`(iMbY << 4) * stride + (iMbX << 4)`, which is a cursor at `(iMbX << 4, iMbY << 4)`
anchored on the plane's padded origin — the address `pCurY` held.

**Results.** `VaaBackgroundMbDataUpdate` is a **safe fn** — the cascade found it and
retired its allow. `copy_shim` had one caller family and dies with it, so
`common/copy_mb.rs` carries `#![forbid(unsafe_code)]` and `common/` is down to
`mc.rs` alone. Tracking 412 -> **403**; ratchet `raw_ptr` -26, `unsafe_block` -10,
`unsafe_fn` -9, no per-file increase. Two size pins moved with measured numbers
(`SVAAFrameInfo` 360 -> 520, `SVAAFrameInfoExt` 1376 -> 1536); both are drift
trackers rather than ABI contracts — this struct has been off the C++'s 264 since
Phase 6 session B, and `repr(C)` came off at T6.F3.

## F254 — what remains: 22 slice cursors over a picture the fork writes, and the aliasing referee that does not exist

The source-plane *read* path is still

    layer_enc_pic(layer)?.plane(i).cursor(x, y)   ->   PlaneCursor { buf: &[u8], .. }

at **22 call sites** — `svc_mode_decision.rs` (10), `svc_base_layer_md.rs` (11),
`md.rs` (1). Each forms a shared slice over the **whole plane allocation** of a
picture that F117's path writes in-fork, and `bEnableBackgroundDetection` is `true`
by default (`param_svc.rs:293`). A whole-plane shared claim races a concurrent write
to any byte inside it, where the raw pointers this family replaced were byte-precise
and disjoint per worker.

**This predates S9 and was not introduced by it.** S9.0a joined the 22 by giving its
own new path a slice-based view; S9.0b corrected that one to the shared seam. The
other 22 stand.

**The blocker is an instrument, not effort.** The `bg` preset is a *byte* referee
and it passes — a data race need not move a byte, which is the entire reason the
Miri lane exists. But Miri runs `--lib` unit tests and never the diffharness sweeps,
so the one configuration that exercises the background write sits outside the one
instrument that could see the aliasing. **Nothing currently in the battery can fail
on this.**

So the next session's first step here is a targeted two-thread Miri probe over one
source plane — one thread copying as `VaaBackgroundMbDataUpdate` does, another
reading as the mode-decision path does — with its control seen red, on the model of
the three probes already in `svc_encode_slice.rs`. Converting 22 sites before that
probe exists would be converting on faith; this session found three design errors in
this family already, and each was caught by a measurement rather than by care.

## F255 — three of step 3/5/7a's "conversions" were deletions, and the tree had already recorded that they were dead

Steps 3, 5 and 7a retired 38 allows, and **fewer than half were conversions.**

| what | how it went | why |
|---|---|---|
| `WelsIDctT4Rec_c`, `WelsIDctFourT4Rec_c`, `WelsIDctRecI16x16Dc_c` | deleted | slots deleted in S18 (F138/F139: installed, asserted, never called); only the differential tests drove them |
| `shim_wh`, `McLuma_c`, `McChroma_c` | deleted | their three call sites build cursors now, on a pattern the same function already used |
| `WelsSetMemUint16_c`, `WelsSetMemUint32_c`, `WelsSetMemMultiplebytes_c` | deleted | the third had **no caller at all** and was the only caller of the other two |
| `WelsInitEncodingFuncs`'s allow | deleted | its `unsafe` block wrapped `&mut *pFuncList` on a parameter that has been `&mut` since the table flip |
| 9 slice-argument validators | converted | `as_mut_ptr() as *mut i32` over a `[u32; 35]` — a type pun; `v as u32` is the same bits |
| 11 `svc_enc_slice_segment` test allows | deleted | vestigial `unsafe` blocks around calls to those nine |

**The generalisation, and it is S18's rule pointed at `unsafe` rather than at dead
code:** before converting a raw body, ask whether it has a caller. Three of these
families were kept alive by nothing but their own differential test or a stale
import, and a conversion would have carefully preserved code the tree had already
decided it did not need. `WelsInitReconstructionFuncs`'s own comment said so in
plain words — *"the kernels stay: the differential tests drive them by name"* —
which is a statement that they are otherwise unreachable.

**And a correction to this session's own accounting.** An earlier report classified
`svc_enc_slice_segment.rs` as "11 of 20 test instruments" that would stay. They were
not instruments; they were `unsafe` blocks wrapping calls that went safe under them.
So I asked the compiler whether that generalised, instead of guessing twice:
`unused_unsafe` is not suppressed in this crate (`lib.rs:34` deleted the allow
because "it suppressed nothing"), and tree-wide it found exactly **two** unnecessary
blocks. The other **38** `instrument(test)` allows outside `src/api/` are genuine —
they read a raw pointer *as* the instrument, like the decoder's `data_ptr` reach
tests. There is no test-allow harvest waiting.

**`common/` is sealed.** `copy_mb.rs` and `mc.rs` were the last two, and
`common/mod.rs` now carries a subtree `#![forbid(unsafe_code)]` so a new file
inherits the rule. That is one of the plan's end-state milestones reached.

## F256 — step 7's remainder is the fork seam, and partial conversion inside a body retires nothing

`svc_enc_slice_segment.rs` went 20 -> 0 because its nine bodies converted
*completely*. The other two files of step 7 do not behave that way, and the
measurement says why.

Stripping `slice_multi_threading.rs`'s declarations leaves **79 raw dereferences and
61 unsafe calls** across 20 bodies — about seven blockers each. The named callees:

| blocker | count |
|---|---:|
| `current_layer` | 17 |
| `slice_in_layer` | 6 |
| `ctx_dq_layer` | 5 |
| `WelsLoadNalForSlice` / `WelsUnloadNalForSlice` | 8 |
| `InitOneSliceInThread`, `WelsCodeOneSlice`, `thread_bs_buffer`, … | the rest |

**F240's named lever was built and it paid nothing yet.** `current_layer_ref(ctx)
-> Option<&SDqLayer>` is now in the tree, expressible with no `unsafe` at all
exactly as F240 priced it, and ten call sites use it. **Zero allows retired**, because
an allow comes off a body only when its *last* raw operation goes — the same rule
F248 stated for signatures, one level down. Partial conversion inside a body is
preparation, not progress, and should be reported as such.

**What actually blocks the seam.** `slice_bank_root` hands out a raw into the slice
bank *by design*: F71's note records that `&mut Vec<SSlice>` is a `Unique` retag over
the `Vec`, that **every worker resolves bank 0** in the fixed slice modes, and that
two of them retagging it at once is a data race even though neither writes the `Vec`.
So the slices workers write are reached through raws deliberately, and converting
them is the same problem `RecPicView` solved for the reconstruction picture: a shared
view whose cells make concurrent writes lawful, plus a targeted two-thread probe with
its control seen red.

That is design work of the kind D-mt-3 exists for, not a mechanical flip — and the
brief says so itself ("Anything here that changes what worker threads share gets its
own targeted two-thread Miri probe"). It is the largest single design problem left in
the plan, and it should open a session rather than close one.

## F257 — the Miri baseline is read newest-**first**, so S8 advanced nothing by appending, and the number two sessions compared against was three closes stale

S8's brief opened on an instrument debt: `miri_wall_baseline.txt` "still reads an
older session's close". S8 duly wrote a new line — and **appended it**. The gate
reads the baseline with

    data=$(grep -v '^#' "$base" | awk 'NF {print; exit}')        # gates.sh:206

— the **first** non-comment line. So the appended line sat where nothing looks, and
both S8's and S9's six gates went on comparing against S5's `cpu=1151`. That is very
likely how the file went stale in the first place: S6 and S7 were reported as not
advancing it, but an append would look like advancing it and behave like not.

Fixed by ordering: closes are **prepended**, the file says so in its own header, and
S8's line is kept beside S9's with its ratio recomputed against the number it should
have used.

**The measurements, restated honestly.** S9's lane is `cpu 1419 s` (wall 559 s)
against S8's `1224 s` — **ratio 1.16**, not the 1.23 the gate printed against the
stale line. S8's own was 1224 against S5's 1151 — 1.06. Neither trips the 1.3 wire
and both are real increases; the lane also gained a test at S9.0b. Six gates ran on
one machine this session, so the next close should re-measure before reading a trend
into two points.

**The rule this earns, and it is the third instance today.** S8 fixed the tracking
metric because it counted comments (F250); S9 found the ratchet baseline needed a
field-by-field diff before regeneration, twice; and now the Miri baseline turns out
to have been written in a direction nothing reads. **An instrument's output is not
evidence that its input was consumed** — after advancing a baseline, read it back
the way the tool reads it.

## F258 — the source-plane referee exists now, and its control is red at *one* round: F234's calibration rule does not generalise

F254 recorded that **nothing in the battery could fail** on a source-plane aliasing
mistake. That is no longer true. `svc_mode_decision.rs`'s
`source_plane_reads_do_not_race_the_in_fork_background_copy` puts two scoped threads
over one `SPicture`'s luma plane, reached through the production `RoPicView`: a
writer running `VaaBackgroundMbDataUpdate`'s luma copy into macroblock 0, a reader
running the mode-decision 16x16 source fetch from macroblock 1. It is named under
`encoder::`, so the Miri lane runs it from now on, and it is **entirely safe code** —
no `allow(unsafe_code)`, unlike the four probes in `svc_encode_slice.rs`.

**Two controls, both seen red.**

| control | verdict |
|---|---|
| reader respelled as the route the remaining sites use — `pic.plane(0).cursor(16, 0)` | **RED**: "Data race detected between (1) non-atomic write on thread `unnamed-2` and (2) **retag read of type `[u8]`** on thread `unnamed-3`", naming `Vec`'s own deref as the second access and `RecCursor::set` as the first |
| reader pointed at the writer's *own* macroblock, still through the seam | **RED** — cells make a concurrent write lawful, not an overlapping one correct |

The first control is the one that matters: **the reader touches no byte the writer
touches.** What races is the whole-allocation shared claim the `&[u8]` makes on its
way to a byte-precise read. That is F254's defect in miniature, and it is why the fix
is the seam and not a narrower slice.

**The calibration, and it contradicts F234 — which is the finding.** The
partition-counter probe needed 200 rounds because at 8 its control came back green,
and its comment says the number is load-bearing. I did not inherit that number; I
measured. **Both controls here are red at 1, 8 and 32 rounds** (the first also at 4,
the second also at 200) — red at a *single* round. The reason is access density: one
round is sixteen 16-byte rows against a sibling claiming the whole allocation, where
F234's probe raced two byte-precise scalars and needed a lucky schedule. So the count
was chosen for **cost**, not sensitivity: 64 costs 16.8 s under Miri where 200 costs
35.8 s, and still leaves 64x margin over the smallest count seen red.

**The rule.** A calibration is a property of the probe, not of the project. F234's
"the round count is load-bearing" is true of F234's probe and false of this one, and
copying it would have cost 19 s on every session gate forever while *reporting* a
margin that was never measured. Calibrate each probe against its own control.

## F259 — step 2 is not a call-site edit, it is a kernel-family migration, and the constraint that makes it one is a function pointer

The brief priced step 2 as "the 22 slice-cursor sites migrate to the shared seam …
each moves onto `SharedPlane` views, the shape S9.0b established". I measured the
enumeration and then the blast radius, and both are wrong in the same direction.

**First, the count.** F254's enumeration is 22 — `svc_mode_decision.rs` 10,
`svc_base_layer_md.rs` 11, `md.rs` 1. Re-measured, non-comment `layer_enc_pic` call
sites are **21**: 10 / **10** / 1. (Those 21 sites derive 27 plane cursors between
them, so neither 21 nor 22 is the number of *cursors*; say which is meant.)

**Second, and this is the finding.** Flipping all 21 to `layer_enc_view` compiles to
**32 type errors in two classes** — 28 `&PlaneCursor` -> `&RecCursor` and 4
`&PaddedPlane` -> `&SharedPlane`. Those are not the work; they are the entrance to
it. The cost kernels the source cursors feed are reached through
`PSampleSadSatdCostFunc = fn(&PlaneCursor<'_>, &PlaneCursor<'_>) -> i32` — a
**function pointer**, which cannot be generic — and `svc_motion_estimate.rs` indexes
it by a runtime `block_size`, so the slot cannot be bypassed.

I checked whether the two operand positions could take different types, since a
function pointer permits that. They cannot:

| position | receives |
|---|---|
| 1 | enc plane (`md.rs:1647`), scratch buffer (`svc_mode_decision.rs:1194`) |
| 2 | enc plane (`svc_mode_decision.rs:1194`), scratch (`md.rs:1474`), **reference plane** (`md.rs:1647`, and every `sad_fn(&cEnc, &pRefPlane.cursor(..))` in the ME) |

So position 2 must be one type serving enc, scratch and reference alike. The moment
the source operand becomes `RecCursor`, **the reference plane must follow** — and the
reference cursor is what `MeRefineFracPixel` hands to `common/mc.rs`'s half-pel
filters. `mc.rs` has **29 functions taking `&PlaneCursor`**, reads them through
`row(dy, dx0, len)` at 18 sites with *runtime* lengths, is shared with the decoder,
and carries measured comments on its loop shapes ("*A zipped column walk, not
indexing*", "*A rolling window, not six fresh `row` calls per output row*").

**So step 2's true scope is: the SAD/SATD/ME/MC kernel families migrate to a cursor
type a shared cell view can produce.** That is a session of its own and it wants the
bench referee, which the plan defers to the exit battery. It is not 21 call sites.

**What was measured, and what it makes free.** Two objections that could have killed
the design are dead, both by measurement rather than argument:

* *`common` may not depend on `encoder`* — a rule this tree records at
  `intra_pred_common.rs:118` and reached by making kernels generic over
  `safe::plane::RefSamples`. That works here too: `sad_common.rs` went generic and
  still serves `processing/scene_change_detection.rs`, which feeds it `PlaneCursor`
  over borrowed `&[u8]` and **cannot** produce a `RecCursor` (`over_owned` needs
  `&mut`). Any design that made the kernels concrete would have broken that caller.
* *cells and by-value rows cost speed on the hottest kernel* — they do not. A
  standalone 16x16 SAD over a 1952-stride plane, 4096 anchors x 300 iterations,
  timed three ways: today's inherent `&[u8; N]` walk **33.8 ms**, a generic
  by-value walk over a slice **33.8 ms**, the same over cells **33.9 ms** —
  ratios 1.000 and 1.004. The bounds-check *fold* is what matters, not the row's
  ownership, and `&[Cell<u8>]` can be lent so the fold survives.

The working design is therefore known and priced: `RefSamples` gains a folded
`row_blocks<const N>` returning `[u8; N]`; `sad_common` goes generic; `sample.rs`,
the two cost slots and `pfCopyBlockByMode` go concrete on `RecCursor`; scratch
operands use the existing `RecCursor::over_owned`; the reference plane gets
`layer_ref_view`; `mc.rs`'s reachable filters go generic over a runtime-length row
accessor that does not yet exist. **The work was built to the `mc.rs` boundary,
measured, and reverted** — an incomplete type migration cannot be committed, and
committing the infrastructure alone is F256's "lever built early that paid nothing".

## F260 — `ctx_param_raw` and `ctx_vpp_raw` were holding up an ordering rule, not a requirement

`ref_list_mgr_svc.rs` went **27 allows to 19** with no new concept — two combined
accessors on §4.6's existing idiom, and one shared twin of an existing raw accessor.
What the conversion actually removed is more interesting than the count.

Three LTR bodies opened with a field-precise `addr_of_mut!` cursor off
`ctx_param_raw` held across a later whole-context reborrow — **F239's shape**,
single-threaded and sweep-invisible. They survived it by *ordering*, and said so in a
comment (T9.H3): "every raw root first, the `&mut`-shaped LTR borrow last, so it
coexists only with derefs of raws into other allocations and never with a second use
of `pCtx`". Three more bodies reached the preprocess through `ctx_vpp_raw` with a
comment explaining that the slot read made the `&mut` receiver and the shared list
borrow "borrows of two different owners".

Both comments are true. Both describe a **disjointness the compiler could have
granted**, refused only because two accessor calls each claim the whole context.
`ltr_family_mut` projects five fields at once (parameter slot, VAA, reference list,
LTR state, `bRefOfCurTidIsLtr`); `vpp_and_ref_list_mut` projects two. After that
there is no order to keep, and the rule is enforced by the compiler rather than by a
comment that the next editor may not read.

**The generalisation.** A hand-maintained ordering rule in a comment is a *symptom*
of a missing combined accessor, and it is the exact precondition F239 punishes: the
defect fires when someone reorders two statements that look independent. Where this
plan finds such a comment, the accessor it is standing in for is the conversion.

Two smaller notes from the same batch, both worth keeping:

* **`ctx_sps`'s deferral was half-expired.** Its comment justifies the raw return —
  "every caller holds it beside other reaches into the same context; a reference
  would borrow the context for its whole lifetime". True of callers that *keep* it,
  false of the three here that read one field on one line. `ctx_sps_ref` is the
  shared twin those take. A deferral can expire for *part* of its caller set.
* **The cascade found the eighth body, not I.** After the batch,
  `deunsafe_cascade.py` reported `RefStrategyKind::EndofUpdateRefList` newly safe —
  its three arms had all gone safe underneath it. Re-running the cascade *after* a
  conversion batch, which the brief mandates, paid a whole allow here.

## F261 — a ```ignore doc block on a public item is an ignored doctest, and the gate counts them

The first `session` run of S10.5a failed on `cargo test (debug): ignored set is 21,
must be 20 (plan §1.4)` — with no test added or ignored. The cause: a ```ignore
fenced block in the doc comment of a new **public** accessor. `cargo test` collects
doctests from public items, and an `ignore` fence is an ignored doctest.

S10.1's probe carries the same fence and did *not* trip the gate, which is what makes
the rule legible rather than mysterious: that one is inside `#[cfg(test)] mod tests`,
from which doctests are not collected. So the trigger is **public item + ```ignore**,
not the fence alone.

The tree's own convention is already ```text — `ctx_vpp_raw`'s doc uses it for the
Miri verdict it quotes — and ```text is not collected at all. Use it for any
illustration that is not meant to compile. This is a cheap finding, but the gate's
message names a count rather than a cause, and the next session to add a documented
public accessor will otherwise spend the same ten minutes.

## F262 — the remainder is three structural items and nothing else, and the cascade proves it

After S10.5b's accessors landed, the per-file de-unsafe cascade was run over the
seven files holding the bulk of the remainder — `wels_preprocess.rs`,
`encoder_ext.rs`, `encoder_context.rs`, `svc_mode_decision.rs`,
`svc_base_layer_md.rs`, `svc_motion_estimate.rs`, `md.rs`, **114 unsafe
declarations between them**. It converted **one**.

That is not a disappointing number; it is a *measurement of where the work is*, and
it retires a question the plan has carried since S8: whether there is any harvest
left that a mechanical pass could take. There is not. Blocker analysis over every
remaining body attributes the **347** to exactly three structural items:

| gate | what it blocks |
|---|---|
| the **plane family** (F259's re-scoped step 2) | `svc_mode_decision.rs`, `svc_base_layer_md.rs`, `md.rs`, `svc_motion_estimate.rs`, and the `SPixMap` plane roots in `wels_preprocess.rs` + the four `processing/` plugins |
| the **slice bank and the bitstream writer** (F256's step 3) | `slice_in_layer`, `slice_writer`, `slice_bs_buffer` — the last twelve of `rc.rs`, four of `ref_list_mgr_svc.rs`, and most of `svc_encode_slice.rs` / `slice_multi_threading.rs` |
| the **`ctx_*_raw` slot readers** | raw by ruling (F71); the three in-fork ones stay by design, and `ctx_param_raw`'s surviving callers are the `addr_of_mut!` cursor sites S29 protects |

**A tool worth keeping.** The per-file blocker analyzer built for this — strip every
`unsafe` declaration in one file, compile, attribute each remaining error to the body
it lands in, and name its blocker — is what made the attribution above cheap and
exact. It is strictly more informative than the whole-tree cascade, which answers
"can this decl go?" but not "*what* is stopping it". Both matter; the cascade found
`EndofUpdateRefList` that hand-reading missed (F260), and the analyzer is what showed
`ParamValidationExt` was a single parameter flip away from free.

**And a note on how the biggest win was found.** `wels_encoder_ext.rs`'s three
largest bodies carried 56, 68 and 74 raw dereferences. That reads like a campaign;
it was one parameter each. `ParamValidationExt` in particular had **no other
blocker at all** — 70 errors, one cause. Before planning a body-deref campaign,
check whether the derefs share a root: the analyzer's per-body blocker counts make
that a two-minute question.

## F263 — the slice-header writers convert except for the bitstream, and that is the *other* seam step 3 owns

`WelsSliceHeaderWrite` and `WelsSliceHeaderExtWrite` were converted in full and then
reverted. Recording the design, because it is ready and because the reason it did not
land is exactly F256's rule rather than a difficulty.

**What worked.** Their `pCurLayer: *mut SDqLayer` parameter flips to `&SDqLayer`
cleanly: both bodies only *read* the layer (checked — zero writes through it), and a
shared layer borrow is precisely what this file's two probes
(`partition_counters_...` and `slice_banks_take_a_shared_layer_borrow_...`) exist to
certify is lawful while sibling workers write. That flip also takes the `*mut
SDqLayer` out of `PWelsSliceHeaderWriteFunc`'s **slot signature**, and with
`layer_sps_ref` / `layer_pps_ref` beside it, **58 raw dereferences** go.

Two details the conversion turned up, both worth keeping:

* `layer_sps`/`layer_pps` answer null in real states, and the callers have real
  fallbacks (`else 0`, `else 4`) whose comment says the C++ dereferences
  unconditionally and the guards "follow the surrounding style in this port". So the
  safe twins must return `Option` and every use keeps its own fallback verbatim —
  `.expect()` would convert a handled state into a panic.
* `WriteRefPicMarking`'s two raw parameters flip with them, and its null guards
  become inexpressible on T9.H's convention.

**Why it did not land.** The bodies still reach the bitstream through
`slice_writer`, which returns `*mut BsWriter` because its two arms have **different
owners** — the slice's own `sSliceBs`, or the context's `sBsWrite` reached via
`ctx_out_raw`. A safe form must return `&mut BsWriter` borrowed from one of two
places, which needs the caller to hold both mutably; `pCtx` is shared here. That is a
**bitstream seam**, sibling to the slice-bank seam F256 describes, and it belongs to
step 3 with it. Without it the conversion retires **no allow** — partial conversion
inside a body retires nothing — and what it would leave behind is a half-migrated
bitstream path, the same shape step 2 was declined for. So: designed, measured,
reverted, written down.

**Step 3 is therefore two seams, not one**, and the brief's framing should be
amended: the slice *bank* (`slice_bank_root`, the worker-written `SSlice` fields) and
the slice *bitstream writer* (`slice_writer`, `slice_bs_buffer`, `thread_bs_buffer`).
They share callers and should be designed together.

---

*F264–F272 were written at S11's open, paying the debt the S10 continuation left:
eighteen checkpoints (S10.2, S10.3a–e, S10.4–S10.15) landed after the S10 close-docs
commit with their reasoning only in commit messages. F268's number was pre-assigned
by S10.6's own message and is honored here. Each finding names its checkpoints; the
commit messages remain the detailed record.*

## F264 — step 2 landed as F259 priced it, and the first bench in thirty checkpoints caught a real regression (S10.2)

All twenty-one source-plane readers go through `layer_enc_view`'s `SharedPlane`;
`layer_enc_pic` lost its last caller and is deleted with its `unsafe fn`. The
families moved as F259's design said they must: `common/sad_common.rs` generic over
`RefSamples` (forced — `processing/scene_change_detection.rs` feeds it `PlaneCursor`
over borrowed `&[u8]` and *cannot* produce a `RecCursor`), `common/mc.rs` (25 fns)
and `common/copy_mb.rs` (7) generic over `RefSamples + Copy`, `encoder/sample.rs`'s
satd generic over **two independent** operand types so a source-plane `RecCursor`
and an owned-scratch `PlaneCursor` meet without conversion, and four fn-pointer
slots concrete on `RecCursor` behind non-capturing closures (a generic fn item
cannot coerce to a higher-ranked slot type). `common/` gained no dependency on
`encoder/` and its three files stayed at zero allows.

**It retires no allow and that was never the point**: F254's defect class — a
whole-allocation shared claim over a picture the fork writes — is now structurally
unmintable. Nothing on `SharedPlane`/`RecCursor` lends a `&[u8]` over a picture
plane, and the S10.1 probe's red control is exactly the shape production can no
longer reach.

**The bench is what kept it honest.** The first draft gave `RefSamples` a
`row_into(out: &mut [u8])` — a copy, which a cell view must do — and made the
*decoder*, which can simply lend, pay for it: `decode_1080p_bench` fell 358.8 →
334.2 fps on Constrained Baseline (−7%). **The byte sweep was green and always
would have been.** The fix is an associated type `Row<'a>: Deref<Target = [u8]>` —
a borrow for the plane cursors, an owned `RowBuf` only for the cell view — and
354.8 fps came back. Thirty checkpoints without a bench, and the first baseline
caught a real regression for eight minutes of work. The user reaffirmed D-gate-8
with this catch known: the bench debt still clears at E3, whole.

Instrument added: an agreement test in `rec_view.rs` pins the three row routes
against each other on both cursor types, both controls seen red (a one-sample
offset in either the cell or the plane path fails it).

## F265 — the resolver boundary moves, the first safe twin, and two conversions reverted with their reasons (S10.3a, S10.3b)

`InitOneSliceInThread` was `pSlice: *mut *mut SSlice` — the C's out-pointer idiom —
so every caller spent its body on `&mut *pSlice` reborrows and
`addr_of_mut!((*pSlice).sSliceBs)` cursors. It returns `Option<&mut SSlice>` now:
same two outcomes, **one audited derivation instead of one per caller body**. The
`unsafe` stays exactly where the bank's raw root still is (F71's shape).
`WritePrefixNalForSlice` lost seven raw operations whose reason had already left:
its `addr_of_mut!` existed because T9.E2b's callers passed the bs cursor *beside*
the slice and the argument reborrow popped it — no sibling exists any more, so the
raw went with the shape that needed it.

`current_layer_mut(&mut sWelsEncCtx) -> Option<&mut SDqLayer>` is the first safe
twin on the seam-splitting pattern: **the type is the restriction**. A `&mut` to the
context cannot exist while the fork is live (every worker holds `&sWelsEncCtx`), so
a body that can call it is single-threaded by construction, where `current_layer`'s
raw only stated the rule in a comment. `WelsSwapDqLayers`,
`WelsInitCurrentQBLayerMltslc` and `StampLayerIdrFlagForSliceType` (T7.C3's hoist)
took it immediately.

Two conversions were **built and reverted with the reason recorded at the site**:
`EncodeOnePartitionSizeLimited`'s encode loop re-resolves its slice after every
`ReallocateSliceInThread`, and `FinishTask` then stamps `uiSliceConsumeTime` on
**the last slice the partition coded** — a per-iteration `&mut SSlice` cannot carry
that across the loop, and stamping inside it would stamp every slice where the C++
stamps one. And the three fork bodies were each down to the single blocker
`&*job.pCtx`, which is not a step-3 item at all — it retires with the fork seam
(which S10.13 then delivered).

## F266 — `mb_window`'s raw was a borrow-width problem, and the fork's price was being paid single-threaded at scale (S10.3c, S10.3d, S10.3e)

The correction first: I had called `mb_window` "deliberately raw" and stopped.
Nothing outside `src/api/` is deliberately raw — a `// unsafe-cat:` tag records *why
a conversion has not happened yet*, never that it should not happen. Treating a tag
as a stopping point is the exact premise-expiry mistake the plan warns about.

**The width insight.** `mb_window` mints `&mut [SMB]` over a sub-range of a
**shared** `&SDqLayer` so fork workers can each take their own records — a caller
obligation no compiler checks, and for the fork it is real. But
`DynslcUpdateMbNeighbourInfoListForAllSlices` holds the layer `&mut` with no fork
anywhere near it, and still went through `from_raw_parts_mut`. What blocked the
safe form was not aliasing but **borrow width**: the two neighbour walkers took
`Option<&SDqLayer>` — a whole-layer shared borrow that cannot coexist with `&mut`
on the grid — to read `sSliceEncCtx` and nothing else. Narrowed to
`Option<&SSliceCtx>` (15 call sites), the two borrows are of two fields and the
compiler grants them: no raw mint, no `unsafe`, no caller obligation.

**Measured, the mispricing was systematic.** The `slice_bank_root` /
`slice_in_bank` / `slice_in_layer` family has 38 call sites: **29 sit in bodies
whose receiver is `&mut sWelsEncCtx` or `&mut SDqLayer`** — single-threaded by
construction — 7 are genuinely fork-reachable, 2 are the resolvers. `mb_window`'s
eleven: 2 converted, 6 genuinely fork, 3 in its covering test. Both seams split
roughly 4:1 single-threaded. The safe twins (`slice_bank_mut`, `slice_in_bank_mut`,
`slice_in_layer_mut`) are plain `Vec` indexing with `&mut SDqLayer` as the
type-enforced proof of no sibling. **A shared twin would be unsound**: from
`&SDqLayer`, indexing the `Vec` retags the *whole buffer* as shared, which races a
sibling worker writing its own slice — F254's whole-allocation claim one level up,
so `&mut` is the gate rather than `&`.

**Two classifier traps, both caught by tracing.** Matching the enclosing signature
for `&mut` reported `WelsISliceMdEnc` single-threaded — it had matched
`pSlice: &mut SSlice`, not the layer, and the body is a slice encode (fork). And
`Option<&SDqLayer>` on `UpdateMbNeighbourInfoForNextSlice` proves nothing — two
hops of call-tracing put it inside the dynamic-slice encode loop. **Signature shape
is a hint; the call tree is the answer.** Classify before designing.

Two scoped residues: `InitMbInfo` converted but retires nothing — its last blocker
is the stride-table arena (`ctx_mb_index_x`/`_y` over `AllocStrideTables`' raw-carved
`Vec<i32>` regions), a separate seam, scoped not started. And S29's
`WelsCodeOneSlice` premise (`g_pWelsWriteSliceHeader` bodies "derive `&mut` to the
same field") *looks* expired — both bodies use `addr_of_mut!` today — but S29's
evidence was a red from the two full-encode Miri probes, so re-deciding it needs
those probes, not a reading. Noted at the site for a checkpoint that runs them (E3).

## F267 — the pre-fork partition: disjointness the compiler can be shown, and F226's race shape is now inexpressible (S10.4)

`UpdateMbMapForked`'s workers each used to resolve the layer through
`SliceJobHandle`'s raw and mint `&mut [SMB]` from a shared layer under `mb_window`'s
unverifiable "caller owns these records exclusively". That contract is true for a
reason the compiler can be **shown rather than told**: a slice's records are the
contiguous run `[pFirstMbIdxOfSlice[i] .. +pCountMbNumInSlice[i])` and the runs are
disjoint — **a chain of `split_at_mut` is that fact.** The grid is carved on the
calling thread while the layer is `&mut` (the borrow that cannot coexist with a
fork), and each worker is handed its own chunks; nothing crosses the spawn but
`&mut [SMB]` and `&SSliceCtx`, both `Send` on their own merits. Both walker fns are
safe now, and this fork no longer uses `SliceJobHandle` at all.

F226's defect — N workers each taking a `&mut (*pCurDq).sSliceEncCtx` reborrow, a
write-write race with nothing written — is not merely fixed but **inexpressible**:
no worker here can name the layer. And the prose contract became an assert: if the
ranges ever overlap, the carve panics naming the slice and the macroblock, where
`mb_window`'s `# Safety` could only ask the caller to promise.

**The probe was re-aimed at what can still be wrong.** Its old claim (two workers
naming one record race-free by disjointness) is now a program that does not
compile, so `update_mb_map_forked_workers_share_the_layer_without_racing` carves
the grid the way production does and checks the partition arithmetic and the
slice-boundary walk — teeth re-verified by feeding the walker a wrong slice index
and watching the boundary assertion fail. A probe outlives its claim only by
re-aiming at what remains falsifiable.

Coverage fact worth its own sentence: **both diffharness drivers pin
`bUseLoadBalancing` off**, so no sweep row reaches the load-balancing fork;
`load_balancing_completes_frames_with_sane_slice_counts` is that path's only
end-to-end cover, which is why S10.4 leaned on the unit tests and Miri rather than
on 583/583.

## F268 — three write-only fields, and the lint that would have said so is allowed crate-wide (S10.5, S10.6, S10.7)

`SDqLayer::pEncData` (S10.5), `pCsData` (S10.6) and `pSrcPool` (S10.7) were deleted
as **write-only**: completed conversions (T9.C2, step 2) had already moved every
reader elsewhere, and each field had been a per-frame store nothing loads —
invisible, because `lib.rs` sets `#![allow(unused_variables)]` (defensible for a C
port full of unused parameters) and **a field that is only ever written draws no
lint at all**. Three dead raw *bindings* in `svc_encode_mb.rs` (`pPred` twice,
`pPredI4x4`) survived an otherwise-complete conversion the same way. All were found
by grepping for readers; none by the compiler.

Stated plainly: the plan's `raw_ptr` metric counted **15 pointers this session that
no longer had a purpose**, and the way to find them was to ask **"who reads this?"
rather than "how do I convert this?"**. Ask it first for any field the metric
prices as work. `SDqLayer` shrank 840/768 → 808/736 across the trio, each pin
re-measured in both profiles off a temporary probe, per the pin's own rule for
deliberate field changes.

## F269 — the fork seam's blocker, measured: three roots, six pointer types, and the ladder to `sWelsEncCtx: Sync` (S10.5, S10.8–S10.12)

The encode fork never needed a slice partition: after S10.3a its bodies were each
down to `&*job.pCtx`, raw because **`sWelsEncCtx` was not `Sync`**. S10.5 asked the
compiler what blocks `Sync` (a temporary `assert_sync` probe) and got a critical
path instead of a vague "raw pointers everywhere": `SDqLayer` (four fields),
`SVAAFrameInfo` (three pointer families), `SLogContext` (`TraceUserCtx`). Two more
blockers (`SSceneChangeResult::pStaticBlockIdc`, `vBGDParam`'s three) **were masked
by the earlier failures and surfaced only as those cleared** — the probe's first
answer is a frontier, not an inventory; re-ask after every removal.

The ladder, eight checkpoints, with the fix chosen by the *reason* each pointer
existed:

    S10.5   pEncData         deleted        (write-only — F268)
    S10.6   pCsData          deleted        (write-only — F268)
    S10.7   pSrcPool         deleted        (write-only — F268)
    S10.8   pRefList         re-resolved through the context
    S10.9   pCurY/pRefY      identity → usize; the two arrays → Process slices
    S10.10  TraceUserCtx     opaque token → usize, no third unsafe impl
    S10.11  pStaticBlockIdc  ferried address → usize (one hop retyped)
    S10.12  vBGDParam        stored working state → parameters

The distinct kinds, each reusable: **identity-as-pointer** (`pCurY`/`pRefY`'s
whole-tree read is one same-pictures comparison; nothing dereferences them — an
address is plain `Copy` data) costs `Sync` and buys nothing. **Opaque-token-as-
pointer** (`TraceUserCtx`, never dereferenced by this crate) is the same mistake;
`usize` is `Sync` on its own merits and **no `unsafe impl` was minted** — D1 pins
the hand-written impls, and a trace handle is not where to spend a pin. Both
round trips use the strict-provenance pair (`expose_provenance` /
`with_exposed_provenance_mut`) precisely where the pointer that comes back out is
*used* — a bare `as` cast would be a provenance guess, and Miri says so.
**Stored-working-state** (`vBGDParam`'s plane roots and flag array, copied in at
`Process` and read two calls deeper) becomes parameters threaded to the kernels —
T9.X's move, applied to the last plugin holding borrowed state; its erosion walk's
back-reaching pointer cursor became an index cursor over one array. And **an
expired doc premise**: `SComplexityAnalysisParam`'s pair was kept on "no writer in
this port at all — `SetRefMbType` was never ported"; it *is* ported now, and a
self-pointer into a field of the enclosing struct is a reason to *remove* a field,
not keep it. `SetRefMbType` returns `Option<RecPicId>` rather than writing a
`*mut *mut u32` out-parameter.

`pRefList` (S10.8) was the one **live** field: 68 tree-wide mentions turned out to
be four real ones. `layer_ref_pic` re-resolves `ctx.ppRefPicListExt[did]` on the
layer's **own** `sLayerInfo.sNalHeaderExt.uiDependencyId`, not the context's
current one — load-bearing under multi-layer SVC, where the frame loop advances
`pCtx.uiDependencyId` (the `dl` preset is what would have caught the wrong choice).
The two deblocking null-guards went as **subsumed, not removed**: a null `pRefList`
means the stamp never ran, and `pRecView` — set in the same body, never cleared —
is `None` in exactly that state, with both guards sitting immediately above a
`pRecView` guard. And the cost worth budgeting elsewhere: the reference picture now
visibly borrows from the context, so nine bodies and **two dispatch slot types**
(`PInterMdFunc`, `PInterFineMdFunc`) had to say the context and `SWelsMD` share a
lifetime. The tie was always true; the raw field was hiding it.

Nothing asserted `Sync` mid-campaign: at S10.12 the property became *true*, and
claiming it (the borrow crossing the spawn) was gated on its own as S10.13.

## F270 — a grep that cannot see the name it hunts: S10.10's absence claim, corrected by S10.11

S10.10's message said of the block-static ferry chain that "the last of those is
write-only". **Wrong**: `SVAAFrameInfoExt::pVaaBestBlockStaticIdc` is dereferenced
four times in `SetBlockStaticIdcToMd`. The check behind the claim was a grep for
`pBestBlockStaticIdc`, which does not match `pVaaBestBlockStaticIdc` — the
prefixed name does not contain the shorter one — so the four sites were invisible
to the instrument and the absence was reported as a fact. The standing rule ("a
claim that something is absent gets its own grep") is necessary but not
sufficient: **the grep must be shown to cover the aliases of the thing**, and a
field reached through a chain of differently-named hops has aliases by
construction.

The conversion S10.11 then wrote is for a chain that *is* dereferenced: only the
ferry hop (`SSceneChangeResult::pStaticBlockIdc`) is retyped to a held address,
both neighbours keep their pointer types, the dereference site is untouched, and
the round trip is the sanctioned strict-provenance pair precisely *because* the
far end dereferences. The family stays `SCREEN_CONTENT(dormant)` — F177: the port
never allocates an `SVAAFrameInfoExt`, so every such pointer is null today and the
deref unreachable; the code is written to be sound if that ever changes.

## F271 — the second audited `unsafe impl` is deleted, and an ordering comment became a compile error (S10.13)

`unsafe impl Send for SliceJobHandle {}` is gone — a **reduction of the stated end
state**, which had pinned two hand-written impls (D1) and kept both. The handle
carried `*const sWelsEncCtx` for exactly one reason: `sWelsEncCtx` was `!Sync`, so
`&sWelsEncCtx` was not `Send` and could not cross `thread::scope`'s spawn. With
F269's ladder complete the auto impl applies — a shared reference to a `Sync` type
is `Send`, **checked by the compiler rather than asserted by hand**. The three
numbered arguments that justified the impl (disjointness by static index,
order-based assembly, the buffer-count bound) are all still true and still worth
reading; none is load-bearing, and the obituary where the impl stood says so.

The instructive side effect: `StampLayerIdrFlagForSliceType` took
`&mut sWelsEncCtx` *after* the handles were built, which no longer compiles when
the handles hold `&sWelsEncCtx` — so the stamp moved before them. T7.C3 required
"before anything spawns" and that is still exactly what happens; the ordering is
now **enforced rather than described**. The general rule: when a seam converts
from raws to borrows, prose scheduling constraints upgrade into borrow errors —
read what newly fails to compile as *found constraints*, not as obstacles.

A smaller calibration in the same checkpoint: `SliceJobHandle::new` reached
`pSliceThreading` through `ctx_slice_threading_raw`, whose slot-read spelling
exists so a *worker's* kept pointer survives later context retags (F71). Two
`debug_assert!`s on the calling thread need none of that — `as_deref()` is the
whole of it. **Match the derivation's strength to the use**, not to the strongest
user of the field.

D1's pin is now one: `rec_view.rs`'s `Sync for SharedCells`, the reconstruction
seam's genuinely unprovable byte-disjointness claim, refereed by the probes in
`svc_encode_slice.rs`. That is the tree's single remaining `unsafe impl`.

## F272 — with the seam gone the slice core is ordinary work, and the last structural item is scoped (S10.14, S10.15)

The two checkpoints after the seam fell are deliberately unremarkable, which is
the finding: **nothing in the slice core needed a design any more.** The NAL
load/unload pair went safe because nothing kept them `unsafe` once
`EncodeOneSliceInJob` stopped needing them raw — the cascade found it.
`PDeblockingFilterSlice`'s slot takes `&SDqLayer`: both targets only *read* the
layer (reconstruction writes go through `pRecView`'s cells, macroblock records
through `mb_window`), so the raw slot was charging the fork's price to bodies that
need a `&` — the same shape as F266, at a dispatch slot. Six raw parameters
flipped to references, each with a null-guard whose own comment already said the
arm was unreachable (T9.H's convention: on a reference the guard is not dead, it
is **inexpressible**); four bodies stopped resolving the layer raw to *read* two
fields (`current_layer_ref` is the whole fix). Ten bodies in a batch, 337 → 327.

What remains in the slice core is **one thing**: the bitstream seam.
`slice_bs_buffer` hands a worker `&'a mut [u8]` carved out of a *shared* context,
with 25 call sites across the entropy coders (`svc_encode_slice.rs`,
`svc_set_mb_syn_cavlc.rs`, `svc_set_mb_syn_cabac.rs`), and `thread_bs_buffer` is
the same shape (it is `WritePrefixNalForSlice`'s single remaining blocker). The
answer is the same pre-fork partition `UpdateMbMapForked` already uses (F267) —
disjoint `&mut [u8]` ranges carved before the spawn, carried in `SliceJobHandle`
(possible since S10.13), threaded down the writer chain instead of re-derived
from the context inside it. A session's work, not a batch's, and the last
structural item in the encoder.

## F273 — the 2.16 settles at 2.09, it is the encode loop itself, and E3's probe pair inherits the bill

S10's continuation left the session-level Miri lane unsettled: an in-session gate
read **2.16** against the 1.3 tripwire, and S11's brief (the only record — the
reading was never committed, and **no committed change matches the "fix" it says
was applied**) ordered one settle run. The settle ran at S11's open on the
S10.15 tree plus comment-only edits: **299 passed / 0 failed, cpu 3041 s against
the S10 close's 1458 s — ratio 2.09 [CPU, F170]**; wall 1166 s against 540 s =
**2.16**, which is where the brief's number came from (it quoted the wall ratio).
The reading **reproduces**; this is not machine noise.

**Attribution, from the lane's own shard logs.** The lane is four concurrent
shards, and the cost is not where the tests are:

    shard cavlc        1 test    1147.90 s   (encode_loop_runs_with_cavlc)
    shard sizelimited  1 test    1063.66 s   (encode_loop_runs_over_size_limited)
    shard enc          149 tests  352.04 s
    shard other        148 tests  574.11 s

The two full-encode fork probes are **73% of the lane's cpu**, and the cavlc
probe alone pins the lane's wall (1148 of 1166 s — a single test defeats the
lane's parallelism). Their drives are unchanged (no `miri_scaled` or
`#[cfg(miri)]` edit anywhere in the continuation), and at the S10 close the
*whole* lane was 1458 s, so the pair was ≲600 s combined then against 2211 s
now: **the continuation made the encode loop ~4× costlier under Miri.** The
lane's one new test is immaterial (S10.2's row-agreement test: 4.5 s, measured
solo).

**Hypotheses, ranked and deliberately not bisected.** (1) S10.2's kernel
migration: the encode loop's pixel reads now go through cursor rows, and the
cell view's row is an owned `RowBuf` filled by per-element reads — exactly the
op mix a native bench barely sees (F264 fixed the borrowing cursors' copy; the
cell view keeps it by design) and Miri interprets slowest, in the loop that is
pixel-volume-dominated. (2) S10.14/15's parameter flips: more reference retags
per call on the hot slice path under Stacked Borrows. (3) S10.13's handle
borrow. Each bisection point costs a ~20-minute Miri run and **no S11 decision
changes with the answer**, so the split is left unmeasured and this finding is
the record of why.

**What does change.** (a) Every `session` gate this session carries a ~19-minute
Miri lane, wall-pinned by one shard. (b) The baseline is prepended at the
settled level (1166 / cpu=3041), so the tripwire re-arms against today's cost
instead of warning on every gate for the rest of the session; the next genuine
regression reads against 3041. (c) **E3's full-drive fork pair runs the same
path at ~30× the drive** — its ~59-minute pair budget predates this inflation,
and if the ×4 carries, the pair is a 3–4 hour item. Re-measure one probe at
`MIRI_FULL=1` *before* scheduling E3's battery, and budget from the measurement,
not from the baseline file's table.

## F274 — S10.15 formed a `&mut` to layer state inside the fork, against the comment directly above the call — and no gate between here and E3 could see it

`WriteRefPicMarking`'s S10.15 flip made its fourth parameter
`pNalHdrExt: &mut SNalUnitHeaderExt`, and both call sites — the slice-header
writers, which every worker runs on every slice — formed `&mut *pNalHead` over
`pCurLayer.sLayerInfo.sNalHeaderExt`: **one layer-owned struct, N concurrent
`&mut`s**. The body only reads `bIdrFlag`; the reference is unwritten, and that
is no comfort — under Miri's model a `Unique` retag is write-flavoured, so two
workers forming one is itself the data race. The binding's own comment, two
lines above the call, said exactly why the raw had to stay: "T7.C3:
`addr_of_mut!`, not `&mut` — layer state, read-only here, and every worker runs
this. `WriteRefPicMarking` takes the raw pointer unchanged." **The flip
contradicted the sentence directly above it.**

**Why every referee stayed green.** The byte sweeps cannot see it — nothing is
written, so no byte ever diverges (F239's lesson, again). The session lane's
two full-encode probes are **single-threaded** — the genuine multi-threaded
fork pair runs only at `full`/`exit` (F168) — and the lane's synthetic
two-thread probes certify the *layer-body* and *bank* claims, not the header
writers. So S10.14 and S10.15, which re-shaped fork-reachable parameter lists,
ran at a cadence (`cargo test` + sweep + ratchet) with **no aliasing referee at
all for the shapes they changed**. The defect would have surfaced at E3, ~59+
minutes into the fork pair, several sessions and hundreds of commits away from
its cause.

**Proven, not asserted.** The new probe
(`workers_read_the_layer_nal_header_through_shared_borrows`) runs two workers
against one header struct. With the defect's own spelling — `&mut *p` per
worker, unwritten — Miri answers: "Data race detected between (1) retag write
on thread `unnamed-2` and (2) retag write of type `SNalUnitHeaderExt` on
thread `unnamed-3`". Deterministic on the first pair of rounds, so the
calibration is one control run. With shared reborrows it is green (1.30 s;
joins the lane).

**The fix** rode S11.2a's layer flip: `pNalHdrExt: &SNalUnitHeaderExt`, and the
call sites pass `&pCurLayer.sLayerInfo.sNalHeaderExt` — the shape the probe
certifies. The rest of S10.15's batch was re-audited under the same question
and is lawful: `WriteReferenceReorder`'s and `WriteRefPicMarking`'s slice
headers and `WelsWriteSliceEndSyn`'s CABAC context are **slice-owned** (one
worker each), and S10.14's `PDeblockingFilterSlice` takes the layer **shared**.

**The rule this buys.** Before flipping any raw parameter to a reference in
fork-reachable code, classify the pointee's *owner*: slice-owned state may take
`&mut` (per-worker disjoint by the bank's construction); layer- or
context-owned state takes `&` only; and an unwritten `&mut` is not a milder
version of the mistake — the retag is the race. S10.15 flipped six parameters
and got five right; the classification step is what was missing, not care.

## F275 — three seams fell to the same question in one session: *what does this callee actually read?*

S11's conversion mass produced one reusable result, and it is not a technique
but an ordering: **before designing anything for a raw, ask what the callee
reads through it.** Three items the plan had scoped as structural work
dissolved into parameter narrowing:

* **`InitMbInfo`** (S11.2d). S10.3e priced it as blocked by the stride-table
  arena — "a third seam, unrelated to the grid, scoped not started". The body
  read exactly *two* things through its `&sWelsEncCtx`: the layer's macroblock
  X/Y coordinate tables. Given those as bounded slices it is safe, and the
  arena's `unsafe` stays where the arena is.
* **`TryModeMerge`** (S11.2e). Sixteen `.add(i)` dereferences off one raw,
  which existed only to read `sMe8x8` while writing `sMe16x8`/`sMe8x16` —
  three disjoint fields of one struct. Destructured, the compiler grants them.
* **The RC/layer pair** (S11.2c). Three bodies held the layer raw because
  `rc_at_mut` and `current_layer_mut` are two `&mut` borrows of one context.
  Destructured into `pWelsSvcRc` against `iCurDqLayer`/`ppDqLayerList` they are
  disjoint, and `rc_and_current_layer_mut` grants both.

All three are the same shape as S10.3c's `mb_window` finding (borrow *width*)
and S10.3d's slice-bank measurement (single-threaded callers paying the fork's
price). Stated as a rule for whatever remains: **a raw that exists to dodge the
borrow checker is a question about borrow shape, not about ownership** —
narrow the parameter, or destructure the owner into the fields actually used.
The cases where that fails are the ones where a callee genuinely needs the
whole struct *and* a `&mut` field of it, and those are worth recording rather
than forcing: `DynamicAdjustSlicing` takes `&mut sWelsEncCtx` *and*
`&mut SDqLayer` where the layer lives inside the context, and F71's slot-read
is what keeps those two provenances apart. Restructuring that parameter list is
a real design item; converting the layer resolution under it is not.

**The cost of the question is minutes.** Each of the three above took one
reading of the callee and one compile. The plan had them scoped as sessions.

## F276 — the cascade's remaining blocker is a *slot type*, and six of them are already raw-free: the exact chain, measured

S11.2e's survey said 115 bodies carry an allow while holding **no raw syntax
and calling no named unsafe function**. That looked like the cascade
under-reporting; it is not. Those bodies call through **function-pointer slots
whose types are still `unsafe extern "C" fn`**, and an indirect call through an
unsafe fn pointer needs an unsafe context exactly as a direct one does. The
cascade is right and the bodies are genuinely blocked — by a *type*, not by
anything in themselves.

**Six slot types are already raw-free** (measured, not estimated —
`pJudgeSkipFun`, `PDeblockingFilterSlice`, `PMdBackgroundInfoUpdateFunc`,
`PWelsCodingSliceFunc`, `PWelsSliceHeaderWriteFunc`, `PUpdateFMESwitch`; the
other 25 still carry `*mut i16`-style kernel parameters and want the
pointer→slice work first). Dropping `unsafe` from those six compiles down to
**five installee mismatches**, and dropping it from the installees in turn
leaves a blocker set of exactly:

    dereference of raw pointer   3   (svc_mode_decision, the JudgeSkip pair)
    ctx_pic_ref                  2
    mb_window                    1   (the fork's six callers, S10.3e)
    WelsCodePSlice               1 } these two hold ZERO raw lines and are
    WelsCodePOverDynamicSlice    1 } blocked only by their own MD callees
    WelsISliceMdEnc              1   (current_layer + a slice-header cursor)
    WelsISliceMdEncDynamic       1   (same, plus a slice-context cursor)

That is the whole of it. **Attempted and reverted this session** rather than
half-landed: the change does not compile until the chain's leaves convert, and
a leaf conversion mid-batch would have gone in without its own byte referee.

**Why this is the highest-leverage item left.** Every one of those 115 bodies
retires behind this chain, and the chain is seven named functions deep, not a
campaign. The order is bottom-up: `ctx_pic_ref` and the two `JudgeSkip` raw
dereferences, then `WelsISliceMdEnc`/`Dynamic`'s two cursors (both the
`addr_of_mut!((*pSlice).sSliceHeaderExt)` shape S11.2e's `TryModeMerge` turned
out to be — a field cursor held across whole-struct reborrows, which
destructuring answers), then the slot types, then the installees, then the
cascade takes the rest in one pass. `mb_window`'s six fork callers are the one
genuinely structural member and can stay raw behind their own tag if the rest
lands without them.

