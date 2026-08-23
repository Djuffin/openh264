# Phase 9 — Session D: the macroblock cache, and the arena problem

## What this project is

This repository holds the openh264 video codec in C++ (under `codec/`) and a Rust port of
it (the crate at `rust/crates/openh264-rs/`). The Rust crate is a drop-in replacement for
`libopenh264`: same C API, same output bytes, same error codes. It began as a near-literal
translation full of raw pointers, and has been made safe piece by piece, always keeping
the output byte-identical. The decoder is finished — every decoder file compiles under
`#![deny(unsafe_code)]` with essentially no exceptions. The encoder is what remains.

Every encoder file *already* carries `#![deny(unsafe_code)]`. It compiles only because
each remaining unsafe item is marked with `#[allow(unsafe_code)]` and a comment tag naming
the family of work that will remove it. **Phase 9 removes those tags by making the code
genuinely safe.** You are session D.

## What session D is about

The encoder encodes one macroblock at a time (a 16×16 pixel square). While it works on a
macroblock it keeps the intermediate results in a scratch structure called `SMbCache`:
the prediction pixels it is trying out, the residual coefficients after the transform, the
zig-zag scanned levels, the non-zero counts, the skip-mode reconstruction. Roughly a dozen
fixed-size arrays, all owned inline in the struct.

Today the encoder reaches into that scratch structure through **raw pointers**. Ten small
accessor functions hand out `*mut u8` and `*mut i16` cursors into its arrays, and the
consumers pass those cursors around, offset them, and hand them to the transform and
quantisation kernels.

**Your job: make `SMbCache` safe — the arrays reached by borrowing, the cursors gone, the
`*mut SMbCache` parameters turned into `&mut SMbCache`.** Along with it come the fourteen
transform/quantisation dispatch-table slots that a previous session traced to this same
structure.

This is the phase's next unblocking move. Two other families (the picture planes, and the
big encoder context struct) are waiting on it, for reasons set out below.

Work the steps in order. Each has a Goal, the Facts you need, what to Do, and the Accept
bar that ends it. Commit as `T9.D1`, `T9.D2`, … . Record findings in
`rust/docs/phase9_findings.md` starting at **F110**. Leave progress notes in
`rust/docs/safety_refactor_log.md` as you go.

---

## Rules that never bend

1. **No behaviour change. Every commit is byte-identical.** The decoded-picture goldens,
   the 2919-row malformed-stream corpus, both benchmark binaries and the diffharness
   sweeps must produce exactly the same bytes and codes before and after every commit. If
   a byte moves, that is a defect in your change — bisect your own small commits to find
   it. Never re-pin a golden.
2. **The unsafe counts only go down.** Run `rust/tools/unsafe_ratchet.sh check` before each
   commit; it fails if any file's count of `unsafe fn` / `unsafe {` / `*mut` / `*const` /
   `#[allow(unsafe_code)]` rose. If a count must rise for a real reason, rebaseline with
   `unsafe_ratchet.sh generate` **in the same commit** and say why in the message.
3. **Gates.** Per commit: `rust/tools/gates.sh commit` (2–3 min — builds every target, runs
   `cargo test` in debug and release, which already covers the conformance streams and the
   malformed corpus, plus the ratchet and a duplicate-type census). At the session close:
   `rust/tools/gates.sh family`, which adds the diffharness sweeps in both build profiles
   (most of an hour) — that is your byte-parity proof. **Do not run Miri, the benchmarks,
   or any performance measurement.** Miri runs once at the end of the whole phase;
   performance is a separate phase.
4. **New code is safe Rust.** If a raw pointer is forced on you by a caller you cannot
   change this session, leave the existing shim in place with its tag and note it for the
   session that owns it. Never introduce a new raw-pointer signature.
5. **Stay in your lane.** Do not touch: anything under `src/api/` (the frozen C boundary);
   items tagged `SCREEN_CONTENT(dormant)`; the `*mut sWelsEncCtx` context parameters (a
   later session, and blocked — see below); the picture-plane pointers (`*mut u8` into
   pictures, a parallel session); `*mut SDqLayer` / `*mut SSlice` parameters (the layer
   family, later).

---

## Background: why the order of this phase is what it is

Two facts, both measured by earlier sessions, decide everything.

**First — the big context struct cannot convert until the scratch cursors are gone.** The
encoder's main state is one struct, `sWelsEncCtx`, with 65 fields. The port's dominant
idiom is: take a raw pointer to something *inside* the context or reachable from it, call
a function that takes `*mut sWelsEncCtx`, then keep using that pointer. Under Rust's
aliasing rules that is sound **only while the pointer is raw** — a raw write through the
parent invalidates only the bytes it touches. The moment such a parameter becomes
`&mut sWelsEncCtx`, entering the function re-tags the *entire* context and invalidates
every derived pointer the caller still holds. This was not theory: a previous session
converted 109 such parameters, every test passed, Miri rejected it, and the whole thing
was reverted. So the cursors must retire first. `SMbCache`'s cursors are a large part of
what is left.

**Second — the dispatch tables are gated on two families at once.** The cost kernels
(sum-of-absolute-differences and friends), the block copies and the transforms are reached
through arrays of function pointers filled once at start-up. A table has one type, so
re-typing a slot converts *all* its call sites at once — and a previous session's caller
census found that almost every one of those call sites pairs a *picture-plane* operand
with an *`SMbCache`* operand. So those tables can flip only after **both** the plane
pointers and this session's work are safe.

The practical consequence, and please hold on to it: **this session does not flip any
dispatch table.** It makes the `SMbCache` half safe. The tables flip in a later commit,
once the plane half lands too. Expect the session to *reduce* raw-pointer counts
substantially while some shims stay in place with their tags — that is the plan working,
not the plan failing.

---

## The shape of the problem: `SMbCache` is an arena

`SMbCache` is defined at `src/encoder/md.rs:421`. It lives inside the slice structure as
one owned field — `pub sMbCacheInfo: SMbCache` at `src/encoder/svc_encode_slice.rs:311` —
so it is not separately allocated and nothing owns it but the slice.

Its storage is all **owned, fixed-size, inline arrays**:

```rust
pub struct SMbCache {
    pub iNonZeroCoeffCount: [i8; 48],
    pub iIntraPredMode:     [i8; 48],
    pub sMbMvp:             [SMVUnitXY; MB_BLOCK4x4_NUM],
    pub sCoeffLevel:        [i16; MB_COEFF_LIST_SIZE],   // residual coefficients
    pub sSkipMb:            [u8; 384],                   // P-skip reconstruction
    pub sMemPredMb:         [u8; 2 * 256 + 16],          // I16x16 prediction ping-pong
    pub sMemPredBlk4:       [u8; 2 * 16],                // I4x4 prediction ping-pong
    pub sBufferInterPredMe: [u8; 4 * 640],               // inter-prediction scratch
    pub sDct:               SDCTCoeff,                   // the transform blocks
    pub SPicData:           SPicData,                    // per-macroblock plane cursors
    pub uiMemPredLumaHalf:  u8,                          // which half of the ping-pong
    // ... flags and small scalars
}
```

Ten accessor functions in `src/encoder/md.rs` (lines 565–650) hand out raw cursors into
those arrays. They all have the same shape:

```rust
pub unsafe fn coeff_level(pMbCache: *mut SMbCache) -> *mut i16 {
    std::ptr::addr_of_mut!((*pMbCache).sCoeffLevel).cast::<i16>()
}
pub unsafe fn mem_pred_luma(pMbCache: *mut SMbCache) -> *mut u8 {
    mem_pred_mb(pMbCache).add(256 * (*pMbCache).uiMemPredLumaHalf as usize)
}
```
The full set: `coeff_level`, `skip_mb`, `mem_pred_mb`, `mem_pred_luma`,
`mem_pred_chroma`, `best_pred_intra_chroma`, `mem_pred_blk4`, `best_pred_i4x4_blk4`,
`buffer_inter_pred_me`, `dct`. Call counts across the encoder: `skip_mb` 17,
`mem_pred_chroma` 16, `dct` 16, `coeff_level` 13, `mem_pred_luma` 9, `mem_pred_blk4` 3,
the rest 1–2.

**Why this is called an arena problem.** A comment already in the tree
(`src/encoder/md.rs:555–570`, written by an earlier session that hit this and backed off)
states the blocker exactly. Two patterns defeat the naive conversion:

* **A callee takes the arena *and* a cursor into it, in one call.**
  `WelsEncRecUV(pFuncList, pCurMb, pMbCache, pRes, iUV)` — `pRes` points inside
  `pMbCache`. If `pMbCache` becomes `&mut SMbCache`, whichever argument is evaluated
  second invalidates the first. **No argument ordering fixes it.**
* **A caller uses a cursor before and after a call that takes the arena.**
  `WelsIMbChromaEncode` (`src/encoder/svc_encode_slice.rs:1747`) runs
  transform → quantise → inverse-transform on the same buffer, so it holds `pCurRS`
  across the `WelsEncRecUV` call in the middle.

Here is one of those bodies in full, because it is the pattern you will be converting
about thirty times (`src/encoder/svc_encode_slice.rs`, `WelsPMbChromaEncode`):

```rust
let pMbCache  = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);       // the arena
let pCurRS    = crate::encoder::md::coeff_level(pMbCache).add(256);   // a cursor into it
let pBestPred = crate::encoder::md::mem_pred_chroma(pMbCache);        // another cursor
let pFunc = ctx_func_list(pEncCtx);
let dct = (*pFunc).pfDctFourT4.expect("pfDctFourT4 unset");
dct(pCurRS,           (*pMbCache).SPicData.pEncMb[1], kiEncStride, pBestPred,           8);
dct(pCurRS.add(64),   (*pMbCache).SPicData.pEncMb[2], kiEncStride, pBestPred.add(64),   8);
WelsEncRecUV(&*pFunc, pCurMb, pMbCache, pCurRS,         1);   // arena + cursor together
WelsEncRecUV(&*pFunc, pCurMb, pMbCache, pCurRS.add(64), 2);
```

**The answers, and both are already established practice in this codebase:**

1. **Disjoint fields borrow independently.** `sCoeffLevel` and `sMemPredMb` are *different
   fields*, so `&mut cache.sCoeffLevel` and `&mut cache.sMemPredMb` can be live at the
   same time — the compiler accepts that directly, and a helper that returns both as one
   tuple from a single `&mut self` works too. Sub-ranges *within* one field (the chroma
   half at `+256`, the ping-pong halves) come from `split_at_mut`, which is safe. So most
   "two cursors at once" sites become ordinary safe code.
2. **Delete the second path.** Where a callee receives both the arena and a cursor into
   it, drop the cursor parameter and let the callee derive it. The same earlier session
   already did this for fourteen callees that took both a slice and a pointer to that
   slice's own cache, and recorded the conclusion: *"a reference is the wrong tool for an
   arena; deleting the second path is the right one."* `WelsEncRecUV` is the clearest
   case — its `pRes` is `coeff_level + 256 + (iUV - 1) * 64`, a pure function of the `iUV`
   argument it already receives, so the parameter is redundant.

---

## The measurement that scopes your session

The project has a detector for exactly this hazard — `rust/tools/q1c.py`. It finds call
sites where a cursor derived from some structure is held across a call that takes that
structure, which is the thing that would become undefined behaviour on conversion. It was
written for the big context struct, and it aims at a type named in one constant:

```python
CTX_TYPE = "sWelsEncCtx"     # rust/tools/q1c.py:99
```

**I re-aimed it at `SMbCache` and ran it. The result is your work list:**

```
65 functions take a `*mut SMbCache` parameter and would retag on conversion.

file                             sites   (A=held cursor, B=argument order)
encoder/svc_base_layer_md.rs        14   A=14 B=0
encoder/svc_set_mb_syn_cavlc.rs      6   A=6  B=0
encoder/svc_encode_mb.rs             1   A=1  B=0

21 hazardous sites in 4 callers, across 14 distinct callees.
  shape A (a cursor held across the call): 21 sites, 5 distinct cursors at risk
  shape B (argument evaluation order):     0 sites

worst callers:  WelsMdInterMbRefinement 11, WelsWriteMbResidual 6,
                WelsMdPSkipEnc 3, WelsEncRecUV 1
most-implicated callees: dct 7, PredSkipMv 2, skip_mb 1, mem_pred_luma 1,
                buffer_inter_pred_me 1, UpdateP16x16MotionInfo 1,
                PredInter16x8Mv 1, UpdateP16x8MotionInfo 1
```

Reproduce it yourself before trusting it (and make this reproducible for the next reader —
see step 1):

```bash
cd /Users/eugene/projects/openh264
sed 's/^CTX_TYPE = "sWelsEncCtx"/CTX_TYPE = "SMbCache"/' rust/tools/q1c.py > rust/tools/.q1c_tmp.py
python3 rust/tools/.q1c_tmp.py            # and --sites for the per-site listing
rm rust/tools/.q1c_tmp.py
```

**Four cautions about that number, and the last two are defects in the tool itself.**

* It is a **heuristic**: a site it does not flag is "no hazard found", not "proved safe" —
  the earlier session was explicitly refused permission to convert only the clean subset
  for that reason.
* **The units differ from what a plain grep gives you**, so do not "correct" one with the
  other. The tool counts *function bodies whose signature binds the type*; a grep for the
  parameter spelling also picks up function-pointer typedefs and the one callee that names
  the parameter `sMbCacheInfo` rather than `pMbCache`. At the time of writing: **66 bodies,
  65 distinct names, ~70 spellings.**
* **It must run from the real tools directory.** It locates the source tree relative to its
  own file path (`ROOT = Path(__file__).resolve().parents[2]`), so a copy placed elsewhere
  silently scans nothing and prints a confident, clean zero. That cost me a wrong reading
  before I noticed the numbers were too good.
* **It keys callees by bare function name, so same-named functions collapse into one
  entry.** That is why 66 bodies report as 65: `WelsRecPskip` is defined twice, at
  `src/encoder/svc_encode_mb.rs:928` and `src/encoder/svc_mode_decision.rs:484`, and the
  second overwrites the first in the tool's table — so a hazard at one may be attributed to
  the other. This project has been bitten by exactly this before, in a different tool
  (`find_stub_bodies.py` hid the decoder's `GetOption` behind the encoder's same-named one).
  Key by name **and file** when you fix it in step 1, and check whether `WelsRecPskip`'s
  hazard attribution changes when you do.

For scale: the same detector aimed at the big context struct reports 287 sites across 68
callers. Twenty-one sites in four callers is a tractable session.

---

## The fourteen transform slots that come with this family

An earlier session converted the five transform/quantisation kernels that had a direct
caller passing an owned array. Fourteen more were left, because they are reached **only**
through the dispatch table `SWelsFuncPtrList`, and their roughly sixty call-throughs all
pass cursors into `SMbCache` — walking cursors like `pRes.add(64)` for the second chroma
quadrant. They were handed to this family for that reason. The slot types are
`PQuantizationFunc`, `PQuantizationDcFunc`, `PQuantizationFour4x4Func`,
`PQuantizationMaxFunc`, `PQuantizationHadamardFunc`, `PScanFunc`,
`PCalculateSingleCtrFunc`, `PGetNoneZeroCountFunc`, `PTransformHadamard4x4Func` and
their siblings, declared in `src/encoder/encode_mb_aux.rs:202–212` and installed in
`WelsInitEncodingFuncs` (`src/encoder/encode_mb_aux.rs:975–1020`).

The safe kernels beneath them already exist and take array slices — for example
`quant_4x4(dct: &mut [i16; 16], ff: &[i16; 8], mf: &[i16; 8])` at
`src/encoder/encode_mb_aux.rs:333`. What stands between is the raw shim wrapping each one.

**Which of these can flip this session:** a slot whose operands are *all* `SMbCache`
arrays or owned constant tables can be re-typed to a safe function pointer once the arena
is safe — the quantisation, scan, non-zero-count and Hadamard slots look like this, but
**check each one's call sites before assuming**. A slot that also takes a picture-plane
pointer — the forward and inverse DCT slots take two `*mut u8` plane operands — cannot,
and waits for the plane family. Step 4 is where you decide this, per slot, from the
callers.

---

## Also in scope, if it comes cheaply: `SMB`

`SMB` is the per-macroblock metadata record (mode, motion vectors, coded-block pattern),
stored in a per-layer array. Its conversion is already well advanced: **71 parameters are
`&mut SMB` today and 42 are still `*mut SMB`**, concentrated in
`svc_set_mb_syn_cabac.rs` (11), `deblocking.rs` (8), `svc_encode_slice.rs` (5) and
`md.rs` (4).

Be careful here: a raw `SMB` pointer is minted by `mb_at(pCurLayer: *mut SDqLayer, kiMbXY)
-> *mut SMB` (`src/encoder/svc_encode_slice.rs:655`), so it is reached **through the
layer**, which belongs to a later family. Convert the `*mut SMB` parameters whose callers
already hold a `&mut SMB` or can take one without touching the layer; leave the rest
tagged, and say in your report which is which. Do not let `SMB` pull you into the layer.

---

## Steps

### Step 1 — Make the detector aimable, and record the baseline
**Goal:** the `SMbCache` hazard list is reproducible by anyone, and the tool stops being a
one-type instrument.
**Facts:** `q1c.py` hard-codes `CTX_TYPE` at line 99 and builds its regexes from it at
import time; it resolves the source tree from its own file path (`ROOT =
Path(__file__).resolve().parents[2]`), so a copy run from elsewhere scans nothing and
prints a clean zero.
**Do:** three things.
(a) Give the tool a `--type NAME` argument defaulting to `sWelsEncCtx`, so
`python3 rust/tools/q1c.py --type SMbCache` produces the table above.
(b) Make the "scanned nothing" case **loud** — if the scan finds no source files, or finds
zero functions taking the named type, exit non-zero with a message rather than reporting a
clean run. A gate that cannot tell "nothing to find" from "nothing found" is not a gate.
(c) Key the callee table by name **and file** instead of bare name, so same-named functions
stop collapsing; then re-read the `SMbCache` numbers and say whether `WelsRecPskip`'s
attribution moved.
Record the `SMbCache` baseline in your log entry.
**Accept:** `--type SMbCache` reproduces 21 sites / 4 callers (and whatever the callee count
becomes once same-named functions stop merging — state both the old and new figure);
running the tool from a copied path fails loudly instead of printing zero; the default
behaviour for the context struct is unchanged, and you say so with its numbers before and
after.

### Step 2 — Fix the 21 hazardous sites, still with raw pointers
**Goal:** every site where a cache-derived cursor is held across a call that takes the
cache is restructured so that it no longer is — *before* any signature changes, so each
fix is small and independently verifiable.
**Facts:** four callers hold all 21 — `WelsMdInterMbRefinement` (11),
`WelsWriteMbResidual` (6), `WelsMdPSkipEnc` (3), `WelsEncRecUV` (1). The two shapes are
described above; the fixes are (a) re-derive the cursor after the call instead of holding
it across, where the call cannot move the data, and (b) delete the redundant cursor
parameter and let the callee derive it, where the callee gets both.
**Do:** take them one caller at a time, smallest first. For each: state in the commit
message which shape it was and which fix you applied. Re-run the detector after each
commit and watch the count fall. **Where a cursor is held across a call that may
*reallocate* or re-stamp what it points into, do not simply move the derivation — that
changes which storage the cursor names. Stop and write it up.** (These arrays are
fixed-size and inline, so this should not arise; if it does, it is a finding.)
**Accept:** `q1c.py --type SMbCache` reports **0 hazardous sites**; every commit
byte-identical; no signature has changed yet.

### Step 3 — The arena's accessors become borrows
**Goal:** the ten raw accessors in `src/encoder/md.rs` are gone, replaced by safe methods
on `SMbCache` that return `&mut [T; N]` / `&[T; N]`, and the `*mut SMbCache` parameters
become `&mut SMbCache`.
**Facts:** all the storage is owned inline arrays; disjoint fields borrow independently;
sub-ranges within one field come from `split_at_mut`. The ping-pong accessors
(`mem_pred_luma`, `mem_pred_chroma`, `best_pred_*`) select a half using a `u8` flag field
on the same struct — read the flag first, then borrow the array, so the borrow does not
overlap the read.
**Do:** convert root-down, in small commits, one accessor family at a time. Where a
consumer needs two arrays at once, borrow the two fields separately (or add one method
returning the pair). Where it needs two ranges of the *same* array, use `split_at_mut`.
Delete each raw accessor once its last caller is gone, together with its
`#[allow(unsafe_code)]` and `// unsafe-cat:` tag. Convert `*mut SMbCache` parameters to
`&mut SMbCache` as the functions below them go safe.
**Accept:** the ten raw accessors are deleted; `grep -c ': \*mut SMbCache' src/encoder`
falls to zero, or every survivor is named in the report with the reason; byte-identical;
ratchet down.

### Step 4 — The transform slots that can flip, flip
**Goal:** every dispatch slot whose operands are now all safe becomes a safe function
pointer, and its shim is deleted.
**Facts:** the fourteen slots and where they are declared and installed are listed above;
the safe kernels beneath them already take array slices. A slot that also takes a picture
plane cannot flip this session.
**Do:** for each of the fourteen, list its call sites and what each operand names (an
`SMbCache` array, an owned constant table, or a picture plane). Flip the ones with no
plane operand: re-type the slot, install the safe kernel directly in
`WelsInitEncodingFuncs`, convert the call-throughs, delete the shim and its tag. For the
rest, leave the shim tagged and name it for the plane family in your report.
**Accept:** every plane-free slot holds a safe function pointer and its shim is gone; the
remainder is listed by name with what each waits on; byte-identical.

### Step 5 — `SMB`, only as far as it goes without the layer
**Goal:** the `*mut SMB` parameters that do not require reaching through the layer become
`&mut SMB`.
**Do:** convert the ones whose callers already hold a `&mut SMB`. Leave anything that
would need `mb_at` (or another layer-owned route) alone and tagged.
**Accept:** the converted count and the remaining count are both stated in the report,
with the reason each survivor stayed.

### Step 6 — If you run short, drop from the end
Drop order: step 5 first, then step 4's later slots (keep whatever you have flipped),
then step 3's smaller accessor families. **Never drop steps 1 and 2** — a reproducible
detector and a zeroed hazard list are what make the rest of this family convertible, by
you or by whoever picks it up.

### Step 7 — Close
Run `rust/tools/gates.sh family` in both profiles. Run `unsafe_ratchet.sh report`. Record
the tag counts before and after:
```bash
grep -rhn 'unsafe-cat:' rust/crates/openh264-rs/src | sed 's/.*unsafe-cat: //;s/ *$//' | sort | uniq -c
```
Write the log entry, and update the session table in `rust/docs/prompts/phase9.md` §8 with
what landed and what the next session inherits. Then report.

---

## What to report back

1. **Commits** — hash and one line each.
2. **The detector** — that `--type SMbCache` reproduces 21/4/65, what the count is at the
   end of step 2, and what you did about the silent-zero failure mode.
3. **The 21 hazards** — each one's shape and the fix you applied, grouped by caller.
4. **The arena conversion** — which accessors went, how the two-cursors-at-once sites were
   expressed, any `*mut SMbCache` parameter that survived and why.
5. **The transform slots** — which of the fourteen flipped, which did not, and what each
   survivor waits on.
6. **`SMB`** — converted count, remaining count, reason for the remainder.
7. **Byte parity** — the `gates.sh family` verdict in both profiles; confirm zero moved
   bytes. Ratchet and tag-count deltas.
8. **Findings from F110**, and — importantly — **any statement in this brief that the tree
   contradicted**. Quote the brief's version and the tree's. Two of the last three sessions
   found the brief wrong about something structural; assume this one is too, and say so
   plainly rather than working around it quietly.
9. **What the next session inherits**, sized from what you saw: which tables are now
   waiting only on the picture planes, and what the `SMB` remainder needs.
