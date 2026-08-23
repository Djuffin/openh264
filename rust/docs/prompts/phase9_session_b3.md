# Phase 9 — Session B3: the plane family, part 1c — the campaign B2 measured

*Self-contained. Read top to bottom once; then work the steps in order. Every count
below was measured at commit `aa14c457` with the command shown beside it — re-run the
command before you quote a number, and trust the tree over this document when they
disagree. Three of the last five briefs were wrong about something structural (F101,
F110, F117/F118); assume this one is too, find it, and say so plainly. Your findings
start at **F120**.*

## What this project is, in one paragraph

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264 (the C++ is
at the repo root, `codec/`). It ships as a drop-in `libopenh264` replacement and must
stay **byte-identical** to the C++ on every stream the gates run — that is the referee
for everything you do. The port started as C-shaped Rust full of raw pointers; Phase 9
is the safety endgame for the encoder: make the code safe so the ~700 individually
tagged `#[allow(unsafe_code)]` items can be deleted. Every encoder file already carries
`#![deny(unsafe_code)]`; each remaining raw site has a comment tag (`// unsafe-cat:
port-raw(Phase 9)` or `cursor`) and the phase retires them family by family. The plan is
`rust/docs/safety_refactor_plan.md` (standing rules §7.6, cited as S-numbers); the
charter is `rust/docs/prompts/phase9.md`; findings are `rust/docs/phase9_findings.md`.

## What session B3 is about

Session B2 repaired the instruments, built the source-picture route, and converted
**one** function to prove it: `WelsMdPSkipEnc`'s three motion compensations now take
cursors (T9.B22), byte-identical, with the coverage *proven* by a planted fault. B3 is
the campaign that route exists for:

1. **Two user decisions execute.** **D-dead-1**: `WelsMdP4x4` / `WelsMdP8x4` /
   `WelsMdP4x8` are deleted (dead in the port, `#if 0` upstream — quote
   `svc_mode_decision.cpp:635`'s guard in the commit). **D-cov-1**: the 22 dead-to-the-
   encoder `mc.rs` shims go — **read the correction below first**, because the decision
   was made on a wrong premise and the corrected execution is cheaper than ruled.
2. **The remaining source/reference readers convert, function by function** — the 62
   `SPicData.pEncMb`/`pRefMb` reads that survive D-dead-1, the motion-estimation
   struct's three cursors, and the 33 `sRefPicView.sPlanes` reads.
3. **The tables flip**: `pfSampleSad`/`pfSampleSatd`/`pfSample4Sad` (with `md_cost`/
   `me_cost` and the 42 handoff signatures), then `pfDctT4`/`pfDctFourT4`.
4. **Three files reach `#![deny(unsafe_code)]`**: `common/sad_common.rs`,
   `encoder/sample.rs`, `common/mc.rs`.

**Not this session**: the reconstruction picture in any form (`SPicData.pCsMb`/`pDecMb`,
`pfIDctT4`/`pfIDctFourT4`, the copy table `pfCopy*`, the intra-prediction tables'
reference side, deblocking, the MT fork) — session C's, under D-mt-3. Converting
`*mut SDqLayer`/`*mut SSlice`/`*mut SMB` parameters — session E's. The context — G–H.
Screen-content (`SCREEN_CONTENT(dormant)`) — Phase 10's. F117's three background-copy
sites — C's, and they are dark to every gate (S57). You touch the layer only by reading
its fields.

## Rules that never bend

- **Every commit is byte-identical.** `bash rust/tools/gates.sh commit` (~2.5 min)
  before every commit. A moved byte is a defect — bisect, do not explain it away. Run
  `gates.sh family` (adds the 535-row sweeps in both profiles, ~2.5 min more) after any
  commit you consider risky.
- **Session close is one command**: `MIRI_SCOPE=encoder bash rust/tools/gates.sh
  session` — sweeps in both profiles **plus the encoder-scoped Miri `--lib` step**
  (D-gate-4, ≈18 min, Miri is 894 s of it). Not optional: session D was byte-identical
  ten commits running and this step found two live UB (F114).
- **No benches, no perf, no unscoped Miri, no differential-test Miri** — phase close
  only (D-gate-1/2).
- **The ratchet only goes down** (`rust/tools/unsafe_ratchet.sh`); any new raw signature
  is tagged the day it is written (D-exit-1); a rebaseline needs its reason in the same
  commit.
- **The commit unit is the signature-reachability closure (S20), and for this family
  the closure includes the *slot* a converted operand feeds (S20's F118 clause)**: a
  `PlaneCursor` cannot enter a raw slot — no `as_ptr` exists, and `root_ptr` is
  `&mut self`, F73's retag — so for any call site the operand and the kernel move
  *together*. What decouples them from the table is **direct call**, not sequencing;
  see "the F118 order" below.
- **Stay in lane.** A blocker outside the lane becomes a finding and you route around
  it.

## Six Stacked-Borrows facts to keep in your head

Byte gates cannot see any of this; the Miri step at your close can, once.

1. **A `&mut T` parameter retags all of `T` on entry** (F66). A raw cursor into `T`
   held by the caller across the call is dead afterwards.
2. **A shared `&T` argument is a protector for the whole call** (F114b, S56). "The
   callee only reads" is about the body; the protector is about the call — walk the
   callee's callees before converting a parameter.
3. **`addr_of_mut!` stops rescuing a derivation once its parent stops being raw**
   (F114a, S29's clause). Under a `&mut` parent, a sibling *safe* borrow of the field
   pops it. Derive raws **at** the call that still needs one; never hold one across a
   safe borrow of the same field. T9.B22's comment block in `svc_base_layer_md.rs` is
   the worked example — read it.
4. **Repeated `&mut` borrows of one picture are a stack, not siblings** (S40, F73).
   `SPicture::planes()` takes `&mut self`; inside the macroblock loop use only the
   shared routes — `layer_enc_pic` / `layer_ref_pic` / `ctx_pic_ref` — and build
   cursors from `&SPicture` per call. Never hold a picture borrow or cursor across a
   call that takes `&mut` on either pool.
5. **A conversion's 535/535 is evidence only if the gate visits the site** (S55's
   clause, S57). For each *new shape* of conversion you land (first direct-call SAD,
   first DCT, first ME rebase), plant a one-sample fault, watch the sweep fail, revert,
   and note the differing char in the commit message — T9.B22 did (char 9223). And a
   site **no** gate runs converts never, not carefully (S57): leave it raw, tagged,
   with the reason beside it.
6. **Run the detector around every family you touch**: `q1c.py --type <T> --kind ref`
   before and after. Since T9.B20 it reports all four shapes — A (cursor held across a
   retagging call), B (argument order), C (raw popped by a safe borrow — F114a), D (a
   protected reference argument — F114b) — and it was calibrated against the tree that
   carried D's two live defects. `--type SWelsME --kind ref` is 0 across 20 bodies
   today; keep it 0.

## The proven route (T9.B22 — copy this shape)

`WelsMdPSkipEnc`'s conversion is the model. The reference picture resolves **once per
use** through the layer; the cursor anchors at the sample the C++ arithmetic lands on;
the destination is a safe borrow of the arena buffer; surviving raws are re-derived
after the borrows end:

```rust
let pRefPicture = layer_ref_pic(pCurLayer).expect("bound");   // &SPicture, shared route
let cRefLuma = pRefPicture.plane(0).cursor(
    kiMbXLuma + sQpelMvp.iMvX as isize,
    kiMbYLuma + sQpelMvp.iMvY as isize,
);
let mut cDstLuma = PlaneCursorMut::new(&mut (*pMbCache).sSkipMb[..256], 0, 16);
mc_luma(&cRefLuma, &mut cDstLuma, sMvp.iMvX, sMvp.iMvY, 16, 16);
// ... later, re-derived AFTER the borrow above ended, never carried across (F114a):
let pDstLuma = std::ptr::addr_of_mut!((*pMbCache).sSkipMb).cast::<u8>();
```

For the **source** picture the route is `layer_enc_pic(pLayer)` (`svc_encode_slice.rs:966`,
built in T9.B21 from `SDqLayer::pEncPic + pSrcPool`); for chroma, `plane(1|2)` and
`<< 3` coordinates. The strides you delete were the same numbers `plane(i).stride()`
returns — `sRefPicView` is stamped from the same picture (`encoder_ext.rs:1952`).

## The F118 order — why most sites do not wait for the flip

`WelsInitSampleSadFunc` (`sample.rs:332`) is the **only writer** of the three cost
tables and its `_uiCpuFlag` is unused; `WelsInitEncodingFuncs` (`encode_mb_aux.rs:859`)
likewise installs `WelsDctT4_c`/`WelsDctFourT4_c` unconditionally. The tables are
constant after init. Therefore:

- A call site whose index is a **compile-time constant** (`pfSampleSad[BLOCK_16x16]`)
  may call `sample_sad::<16, 16>` (or `dct_4x4`/`dct_four_4x4`) **directly**,
  byte-identically, in the same per-function commit that converts its operands. State
  the constant-after-init proof (one writer, flag unused) in the first such commit.
- Only **runtime-selected** reads need the safe table: the `md_cost`/`me_cost` sites (5
  calls) and the variable-index hoists (`[block_size]` 10, `[block]` 4, `[b]` 3 —
  *slot-read* counts by `grep -rn 'pfSampleSad\[' src/encoder` and siblings; the
  census's 37 counts *call sites*, a different unit). Those wait for step 4's flip.
- Motion compensation involves **no table at all** (Phase 4a made it direct): 30 call
  sites remain (33 minus T9.B22's 3) — `svc_base_layer_md.rs` 14, `md.rs` 10,
  `svc_mode_decision.rs` 6.

## The map, measured at `aa14c457`

- **`SPicData` source/reference reads: 68** (`grep -rn 'SPicData\.pEncMb\|SPicData\.pRefMb'
  src/encoder | grep -v ':\s*//' | wc -l`): `pEncMb` 45 (`svc_base_layer_md.rs` 25,
  `svc_mode_decision.rs` 14, `svc_encode_slice.rs` 4, `svc_encode_mb.rs` 2), `pRefMb`
  23 (13 + 10). D-dead-1 removes 6 → **62**. The `svc_encode_slice.rs`/`svc_encode_mb.rs`
  reads sit inside residual-encode bodies as DCT operands — they convert with their DCT
  sites, not before.
- **`SWelsME`** (`svc_motion_estimate.rs:153`): `pEncMb`/`pRefMb`/`pColoRefMb`
  (`:164–166`), written at exactly four places — `InitMe` (`svc_mode_decision.rs:1208`)
  and `svc_motion_estimate.rs:406`, `:659`, `:926`. `iCurMeBlockPixX/Y` are already in
  the struct. `SWelsMD` (which embeds the `sMe` family) is a **stack local per
  macroblock** (`let mut sMd = SWelsMD::default()` — 4 sites), so a lifetime-carrying
  cursor field is *possible*; coordinates + MV need no lifetime and match the carrier
  doctrine. Choose, and say why in the commit.
- **`sRefPicView.sPlanes` reads: 33**; **`pEncData` reads: 6**. Both are per-frame raw
  roots whose picture `layer_ref_pic`/`layer_enc_pic` name; delete each root when its
  last reader is gone.
- **Handoffs: 42** (`python3 rust/tools/phase9_plane_callers.py --handoff`) —
  `pfCalculateSatd`'s slot reads (`svc_motion_estimate.rs:585`, `:630`, `:674` — and its
  *type*, `PCalculateSatdFunc`, carries `Option<PSampleSadSatdCostFunc>` as its first
  parameter), the search entry points' hoisted
  locals, `SFeatureSearchIn::pSad` (`svc_motion_estimate.rs:1400`, SCREEN_CONTENT-
  adjacent but its *type* changes with the slot), and `md_cost`/`me_cost`'s returns.
- **Shims**: `sad_common.rs` 14, `sample.rs` 7 (+ `WelsInitSampleSadFunc` itself, which
  becomes a safe fn), `mc.rs` 28 (6 with live callers, 22 without — F119),
  `encode_mb_aux.rs`'s `WelsDctT4_c`/`WelsDctFourT4_c`.
- **Dark functions** (B2's probe, F117's method — a print that ran never across a full
  sweep): `WelsMdBackgroundMbEnc` and `SvcMdSCDMbEnc` — 6 of the 30 MC sites plus their
  SAD sites. `bEnableBackgroundDetection = false` in the diffharness driver
  (`rust_enc/main.rs:126`). **S57: these stay raw and tagged** unless you take step 6.
- **The stale typedef block** `svc_encode_mb.rs:291–311`: raw duplicates of the flipped
  slot types; several have zero uses. Delete the dead ones with the DCT step —
  verifying each with a grep that includes `tests/` (S18's F119 clause).

## The D-cov-1 correction (measured while writing this brief — the third instrument lesson)

F119 and the decision built on it say the 22 dead `mc.rs` shims are "kernel-level
parity evidence against the C++ reference", and D-cov-1 therefore orders the harness
rewritten to drive the safe kernels before the shims go. **The file says otherwise.**
`tests/kernels_differential_phase2.rs` has no C++ linkage anywhere (`grep -rn 'extern'
tests/` — only the port's own API `c_void` casts); its doctrine, in its own header, is
that raw-vs-safe equivalences are *deliberately short-lived* and are deleted the moment
the raw side becomes a shim onto the safe kernel. The mc equivalences were proven at
`46053993` — exhaustive selector sweeps, every destination byte — and deleted then. The
**only** surviving mc test is `mc_shims_stay_inside_the_spans_they_declare`
(`tests/kernels_differential_phase2.rs:251`): a contract test of the **shims' own span
arithmetic** (the `Reach` restatement), which tests nothing once the shims are gone.

**So execute D-cov-1's intent, which is cheaper than its wording**: delete the 22
shims *and* that span test *together*, in one commit, quoting the file's doctrine and
`46053993`. No harness rewrite exists to do — the parity evidence D-cov-1 meant to
preserve never lived there. If you find this correction wrong — an actual C++ side
somewhere — stop, file it as a finding, and do D-cov-1 as originally worded.

## Steps

### Step 0 — D-dead-1 (one commit)
**Do:** delete `WelsMdP4x4`/`WelsMdP8x4`/`WelsMdP4x8` (`svc_base_layer_md.rs:1094`,
`:1164`, `:1234`) with upstream's `#if 0 //Disable for sub8x8 modes for now` quoted;
regenerate the plane census (its site list shrinks; the tool exits 1 on unknowns —
if that fires, teach it the spelling in the same commit, S58).
**Accept:** the three names appear nowhere (`tests/` included); −4 tags, −6 reads;
byte-identical.

### Step 1 — The campaign (the bulk; one commit per function or tight group)

**Goal:** every remaining source/reference read outside the dark functions goes through
a cursor built at the point of use; MC calls take the safe kernels; constant-index
SAD/SATD and DCT sites call the kernels directly (the F118 order).

Work the list function by function, roughly smallest-risk first:

1. `WelsMdPSkipEnc` — finish it: the three SAD sites and the `pEncMb` reads (the MCs
   are done).
2. `AcceptPskip` (`kpPicData: &SPicData` parameter — the bundle arrives by reference;
   the census's `PARAM_CLASS` table names its operand classes).
3. `CalUVSadCost` + its two callers' `ctx_pic_ref_mut(..).planes()` sites
   (`svc_mode_decision.rs:1842`, `:1899`) → the shared route (fact 4).
4. `MeRefineFracPixel` / `MeRefineQuarPixel` (`md.rs` — 10 direct half-pel MC/avg
   calls into `pBufMe`, plus their `me_cost` SAD sites; `pBufMe` is
   `sBufferInterPredMe`, an owned arena buffer — `PlaneCursorMut::new` over it).
5. `WelsMdInterMbRefinement` (`svc_base_layer_md.rs` — the big one: 14 MC sites, 6
   SADs, ~430 lines). Do it in two or three commits if the closure allows; this
   function is exactly where a rushed job re-creates F114's class, which is why B2
   stopped in front of it.
6. `WelsMdI16x16`, `WelsMdIntraChroma`, `WelsMdI4x4Fast`'s cost sites, and the rest of
   the census's safe-now list (`python3 rust/tools/phase9_plane_callers.py --sites`).

Per-function recipe: (a) `q1c.py --kind ref` on the types the function takes, before
and after; (b) pictures resolve per call through the shared routes; (c) constant-index
cost sites → direct kernel calls; (d) MC → T9.B22's shape; (e) surviving raws
re-derived after safe borrows end (F114a); (f) one planted-fault calibration per new
conversion shape, reverted, char noted (fact 5).

**Accept per commit:** `gates.sh commit` green; byte-identical; ratchet down or flat.
**Accept for the step:** `SPicData.pEncMb`/`pRefMb` reads = only the dark functions'
(count them and say so); the two `planes()`-in-loop sites gone.

### Step 2 — The ME struct stops carrying pointers (one to two commits)

**Do:** `SWelsME::pEncMb`/`pRefMb`/`pColoRefMb` become coordinates (+ the MV the struct
already carries) or lifetime-carrying cursors — decide against the four assignment
sites. **First verify the invariant** the coordinate design assumes: that `pRefMb`
always equals the co-located position offset by the current candidate
(`:659` is `pColoRefMb.offset(mv_y * stride + mv_x)` — check `:406` and `:926` say the
same thing). If any site breaks it, keep an explicit offset field and say so. The
search functions get their planes from `pCurDqLayer` (`PMotionSearchFunc` has it) or as
parameters where they do not; a stride parameter a cursor now carries is deleted only
after **every** caller is read (S54).
**Accept:** the three raw fields gone; `q1c.py --type SWelsME --kind ref` still 0;
sweeps green in both profiles (`st` and `mt` presets drive the searches).

### Step 3 — The cost tables flip (one commit)

**Do:** `PSampleSadSatdCostFunc` → `fn(&PlaneCursor<'_>, &PlaneCursor<'_>) -> i32`,
`PSample4SadCostFunc` → its four-candidate shape (`sample_sad_four`'s signature);
`WelsInitSampleSadFunc` installs the safe kernels and becomes a safe fn;
`md_cost`/`me_cost` return the new type; all 42 handoff signatures; the 5
`md_cost`/`me_cost` call sites and the variable-index hoists build cursors (their
operands are safe after steps 1–2); delete the 14 + 7 shims;
`#![deny(unsafe_code)]` on `common/sad_common.rs` and `encoder/sample.rs`.
**Accept:** both files deny; the slot types name no raw pointer; byte-identical.

### Step 4 — The forward DCT flips (one commit)

**Do:** `pfDctT4`/`pfDctFourT4` → `fn(&mut [i16; 16], &PlaneCursor, &PlaneCursor)` and
the `[i16; 64]` form (two typedefs — one raw type serving two spans is F113's defect);
`WelsDctMb` (`svc_encode_mb.rs:459`) takes safe operands; delete
`WelsDctT4_c`/`WelsDctFourT4_c`; delete the zero-use members of the stale typedef
block (`svc_encode_mb.rs:291–311`), each verified tree-wide. `pfIDctT4`/`pfIDctFourT4`
**stay raw and tagged** — reconstruction, session C's.
**Watch F114a here**: the callers derive raws into `sCoeffLevel` and this slot takes a
safe borrow of that same field. Derive at the call.
**Accept:** the two forward slots safe; inverse slots untouched; byte-identical.

### Step 5 — `mc.rs` closes (one commit)

**Do:** with the 30 live MC call sites converted in step 1, delete all 28 shims **and**
`mc_shims_stay_inside_the_spans_they_declare` together (D-cov-1 as corrected above;
quote the doctrine and `46053993`); re-type `SMcFunc`'s six slots to the safe kernels'
signatures and let `InitMcFunc` install `mc_luma`/`mc_chroma`/`mc_hor_ver*`/`pixel_avg`
(the decoder shares the struct — `decoder_context.rs:1852` — and its field keeps
compiling; verify `abi_guard.rs:184`'s `assert_size!(SMcFunc, 48)` still holds);
`#![deny(unsafe_code)]` on `common/mc.rs`.
**Accept:** `mc.rs` denies; no `Mc*_c` name survives anywhere; byte-identical.

### Step 6 — Optional: light the dark functions

Only if ahead of budget. The 6 MC sites in `WelsMdBackgroundMbEnc`/`SvcMdSCDMbEnc`
convert **only behind a referee** (S57). The referee is a new diffharness preset —
`bEnableBackgroundDetection = true` in **both** drivers plus a `bg` preset in
`sweep.sh` — which also lights F117's copies for session C (S47: every entry point gets
a referee). Expect parity work if rows fail; that is the referee working, and it is a
finding, not a detour. If not taken, the sites stay raw with S57 cited beside them —
which is a fine outcome.

### Step 7 — If short, drop from the end

Drop order: 6, then 5, then 4, then **3 and 2 together** (the flip without the ME
conversion has no compiling middle — F118). Never drop 0–1. Say what you dropped and
what it costs.

### Step 8 — Close

1. `MIRI_SCOPE=encoder bash rust/tools/gates.sh session` → OVERALL PASS. Start it
   before writing the report.
2. Regenerate both censuses; if your conversions introduced spellings the plane
   census cannot classify, teach it in the same commit (it exits 1 now — S58).
3. Findings from **F120**; the log entry; the charter's B3 row.
4. Ratchet deltas and the tag count (the census tool's line is authoritative: **638**
   `port-raw(Phase 9)` + 61 `cursor` at your start; the bare grep over-counts by 5
   prose mentions).

## What to report back

Plain prose. Cover: commits (hash, what moved, ratchet delta); every gate verdict
including the closing session run's Miri tally; the counts re-measured (`SPicData`
reads remaining and where, tags, census columns, files under `deny`); **where this
brief was wrong**, quoting the sentence and what the tree says instead — the D-cov-1
correction above is itself a correction of a correction, so check it the same way; and
what sessions C and E inherit (C: the reconstruction surface plus F117, and whether
step 6's preset exists; E: `q1c.py --type SDqLayer` — 14 hazardous sites in 5 callers
at your start — plus the 32 arena roots and 31 neighbour-bound `SMB` parameters).
