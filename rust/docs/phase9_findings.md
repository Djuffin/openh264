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

## F103 — the coefficient family is 4 kernels, not 25: the other 15 are gated on SMbCache

Session A's brief scopes the coefficient conversion as "the `WelsQuant*`/`WelsScan*`/
`WelsCalculateSingleCtr`/`WelsGetNoneZeroCount` shims", on the premise that
"every input to the coefficient-only kernels is already an owned `[i16; N]` on the
caller side; the raw pointer is a `as_mut_ptr()`/`from_raw_parts` round-trip that
deletes cleanly". That premise holds for four kernels and fails for fifteen.

Of the 19 coeff-pure signatures the census finds:

| | kernels | why |
|---|---|---|
| **directly called** | 4 | caller passes an owned `[i16; N]`; converts cleanly |
| **`SWelsFuncPtrList` slot** | 15 | no direct caller at all outside their own unit tests |

```bash
grep -rn 'Some(WelsQuant4x4_c)\|WelsQuant4x4_c *(' rust/crates/openh264-rs/src
#   encode_mb_aux.rs:1009  f.pfQuantization4x4 = Some(WelsQuant4x4_c);   <- the only
#   encode_mb_aux.rs:1055  (a unit test)                                    real ref
```

The fifteen are reached only through the table, and **the 59 call-through sites do
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
