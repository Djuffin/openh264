# Phase 9 — Session C2: the seam's consumers — 26 sites, three kernel families, and deblocking

*Self-contained. Read top to bottom once, then work the steps in order. Every count below
was measured at the commit this brief landed in, with the command beside it — **re-run
before quoting, and trust the tree over this document**. Session C's brief was wrong
about something structural (F131/F132); assume this one is too, and say what it was.
Your findings start at **F136** — check `grep -c '^## F' rust/docs/phase9_findings.md`
before you believe that.*

## What this project is, in one paragraph

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264 (the C++ is at
the repo root, `codec/`). It ships as a drop-in `libopenh264` replacement and must stay
**byte-identical** to the C++ on every stream the gates run. Phase 9 is the encoder's
safety endgame: every file carries `#![deny(unsafe_code)]`, each raw site has a tag, and
the phase retires them family by family. The plan is `rust/docs/safety_refactor_plan.md`
(rules §7.6, cited as S-numbers); the charter is `rust/docs/prompts/phase9.md`; findings
are `rust/docs/phase9_findings.md`.

## What session C already did, and what it means for you

**The seam exists, is reachable, and is proved.** `encoder/rec_view.rs` holds
`RecPicView` — the reconstruction picture's three planes *and* its four per-macroblock
side arrays — behind **one** `UnsafeCell`-crossing accessor (`SharedCells::cells`) and
**one** `unsafe impl Sync`, both `// unsafe-cat: recon-seam`. Four probes cover it, all
green under Miri, and both threaded ones were calibrated by planting the overlap they
exclude. Read that module's doc comment first: the soundness argument is there and you
inherit it, you do not re-derive it.

**The route is `layer_rec_view(pCurLayer)`.** The layer carries `pRecView`, built once per
frame in `WelsInitCurrentLayer` (`encoder_ext.rs:~2186`) from the same `&mut SPicture`
that stamps `pCsData`. `layer_dec_pic`, `layer_dec_pic_mut` and `SPicData.pDecMb` are all
**deleted** — there is no `&mut` route into the reconstruction picture from inside a frame
any more, and putting one back is a regression the Miri probes will not catch (they stop
earlier, see below), so it is on you not to.

**One consumer is converted, as the pattern.** `WelsRecPskip`
(`svc_mode_decision.rs`, T9.C7): destination is `view.plane(i).cursor(x, y)` with `(x, y)`
from `SPicData`'s `iMbX`/`iMbY` carrier (`luma_origin()` / `chroma_origin()`), operand is
the arena slice, kernel is called **directly** rather than through its `pfCopy*` slot
(F118 — the eight `pfCopy*` entries are installed unconditionally by
`WelsInitEncodingFuncs` and constant after init, so a fixed-size site may bypass the slot
byte-identically). Copy it.

## Your scope: 26 sites

`python3 rust/tools/phase9_plane_callers.py --sites | grep blocked` reports **29**. Three
are `SvcMdSCDMbEnc`'s and are `SCREEN_CONTENT(dormant)` — Phase 10's, not yours; the
census still lists them and F128 says why.

| owner | sites | group | note |
|---|---:|---|---|
| `WelsEncRecI16x16Y` | 5 | 4 idct + 1 copy | `svc_encode_mb.rs:597-619` |
| `OutputPMbWithoutConstructCsRsNoCopy` | 3 | idct | **the in-place family** — `pRec` *is* `pPred`; `idct_t4_rec_in_place` exists for exactly this (F59) |
| `WelsMdInterEncode` | 3 | copy | `svc_base_layer_md.rs:2024-2041` |
| `WelsMdBackgroundMbEnc` | 3 | copy | lit by the `bg` preset, **32-row** teeth, not 48 (F126) |
| `WelsRecPskip` (`svc_encode_mb.rs`) | 3 | copy | **dead code** — F135. See step 0 |
| `WelsMdIntraChroma` | 2 | intrapred | |
| `WelsIMbChromaEncode` | 2 | idct | `svc_encode_slice.rs:1816/1833` |
| `WelsEncRecI4x4Y` | 2 | 1 idct + 1 copy | `svc_encode_mb.rs:706/709` |
| `WelsMdI4x4Fast`, `WelsMdI4x4`, `WelsMdI16x16` | 3 | intrapred | the `pRef` operand is the recon picture |

Plus **deblocking**, which session C touched only at its two roots:
`encoder/deblocking.rs` — 26 unsafe items, 21 tags; `common/deblocking_common.rs` — 19
`unsafe fn`, un-denied.

## The three kernel families, and what each needs

`copy_block_to_view` (in `rec_view.rs`) already covers **every blocked copy site** — 14
by the census, of which **11 are yours** (SCD's 3 are Phase 10's) and **3 of those 11 are
the dead duplicate**. Every reconstruction copy has an *arena* source (`sSkipMb` or
`sMemPredMb`), so a slice and a stride is the right operand shape and no new kernel is
needed. The other two families do:

* **idct** — `encoder/decode_mb_aux.rs`'s `idct_t4_rec` / `idct_four_t4_rec` and the two
  `_in_place` forms, all taking `&mut PlaneCursorMut`. They need a `RecCursor` flavour.
  The two-plane forms only *write* the reconstruction (prediction comes from the arena),
  so `RecCursor::write_row` is enough; the `_in_place` pair reads and writes the same
  block, which `RecCursor::at` + `set` already do. Slots: `pfIDctT4`, `pfIDctFourT4`,
  `pfIDctI16x16Dc` (`PIDctFunc`, `svc_encode_mb.rs:309`) — count the spans before
  flipping (S52/F113 — one type served two lengths last time).
* **intra-pred** — `encoder/get_intra_predictor.rs`'s kernels are already safe over
  arrays; the *operand* that is the reconstruction picture is `pRef`, read not written, so
  these want a `RecCursor` **read** path. The decoder's table of safe `fn` pointers is the
  model (`PGetIntraPredFunc`, `wels_func_ptr_def.rs:51`; the three arrays are
  `pfGetLumaI16x16Pred`, `pfGetLumaI4x4Pred`, **`pfGetChromaPred`** — session C's brief
  called the third `pfGetIChromaPred` and it does not exist).

`common/copy_mb.rs` and `common/intra_pred_common.rs` reach `#![deny(unsafe_code)]` when
their last raw caller goes. Tables flip **last** (F118).

## What is *not* yours

* **The `#[cfg_attr(miri, ignore)]` on the two encoder fork/join probes.** F132 measured
  that the fork shares six families; C closed four, and the two that remain are
  **session E's** (`&mut SMB` over-claiming a record deblocking reads cross-slice) and the
  slice structures' (`SSliceCtx::pOverallMbMap` rewritten in-fork by `AddSliceBoundary`).
  Read F132 before you spend an hour on this: converting all 26 sites will not move
  either probe, because neither verdict is about the reconstruction write. If you want a
  win here, `SM_SIZELIMITED_SLICE`'s map rewrite is the cheaper of the two and the
  mid-row probe (`fork_join_encodes_a_frame_whose_slice_boundary_is_mid_row`) is its
  instrument — but it is a *decision* to take it, not a step.
* **F117's three source-picture copies** (`VaaBackgroundMbDataUpdate`) — still open, still
  refereed by the `bg` preset, design unchanged from B4's handover: record the macroblock
  indices during the loop and execute the copies after the join, or record why the proof
  fails. Nothing session C did blocks it.
* **`pGomCost`'s deletion** (F133) and **`svc_encode_mb.rs::WelsRecPskip`'s** (F135) — both
  are deletion rulings with the evidence attached, and F115 set the precedent that those
  are the steward's call.

## Rules that never bend

* **Byte-identical every commit**: `bash rust/tools/gates.sh commit` (~2.5 min) before
  each; `gates.sh family` (**583 rows** per profile) after risky ones. The seam writes the
  same bytes through a different type — a moved byte is a defect.
* **Session close**: `MIRI_SCOPE=encoder bash rust/tools/gates.sh session` (D-gate-4).
* **No benches, no perf, no unscoped Miri** (D-gate-1/2). **Ratchet only down**; new raw
  tagged same-day (D-exit-1).
* **S20 closures**; the slot rule (F118): operand and slot move together *unless* the slot
  is constant after init, which every table here is.
* **Planted-fault calibration per new conversion shape** (S55/S59), and count coverage in
  *covered rows*: T9.C7's seam-written copy is refereed by **150 of 210** `st` rows, not
  210.
* **Stay in lane**; blockers become findings.

## Steps

0. **Decide the dead duplicate first** (F135). `svc_encode_mb.rs::WelsRecPskip` has no
   caller and holds 3 of your 26. Either get the deletion ruling or convert it as if it
   were live — but do not leave it half-done, and if you convert dead code, say so in the
   commit so the count is not read as progress.
1. **The idct write flavour**, one commit, no consumers: the `RecCursor` forms of
   `idct_t4_rec` / `idct_four_t4_rec` / `idct_rec_i16x16_dc` and the two `_in_place`
   pairs, with a unit test each against the `PlaneCursorMut` forms on the same input.
   Accept: ratchet flat, no behaviour change.
2. **The idct consumers** (10 sites, four owners), direct-called per F118. Planted fault
   once for the two-plane shape and once for the in-place shape — they are different
   conversions and S59 counts them separately.
3. **The copy consumers** — 11 sites, 3 of them the dead duplicate from step 0 —
   through `copy_block_to_view`. `WelsMdBackgroundMbEnc` is the one with a narrow
   referee: quote 32 rows, not 48 (F126).
4. **The intra-pred read path** (5 sites) and the three arrays.
5. **The tables**: `PIDctFunc` → safe over `RecCursor` + `&[i16; N]`; `PCopyFunc` → safe;
   the three intra-pred arrays → `Option<fn(..)>` over safe types. Shims delete;
   `common/copy_mb.rs` and `common/intra_pred_common.rs` reach `deny`.
6. **Deblocking**: the per-MB walk uses view cursors; `deblocking_common.rs`'s edge
   filters gain the `&self`-write flavour; both files reach `deny`. **Fact 5 of session
   C's brief still governs** — the slice-boundary skip is load-bearing, and note that
   deblocking's *own* `uiSliceIdc` reads are F132's round 5, so do not try to make them
   safe here.
7. **Close**: session gate (state the Miri wall-time delta); regenerate both censuses;
   findings; log; charter row; tags and ratchet re-measured, conversions and
   reclassifications never summed (F128).

## What to report back

Plain prose: commits with ratchet deltas; every gate verdict; the consumer table
re-measured (29 blocked → what remains, by owner, and how many of the remainder are dead
code); which kernel families reached `deny`; whether the tables flipped or why not; where
this brief was wrong, quoting the sentence; and what E, F and Phase 10 inherit after you.

---

## Steward's addendum at kickoff (verified against the tree at `be9ab935`)

Everything above was written at session C's close; three things landed after it.

1. **The F107 acceptance re-scope is now a ruling, and it adds one step to your scope**
   (charter §8, F132; the user did not veto). The "not yours" bullet about the fork
   probes stands *except* its `pOverallMbMap` half: **`SSliceCtx::pOverallMbMap` →
   atomics is yours, as step 0a** — fork round 6, **18 mentions** across four files
   (`svc_enc_slice_segment.rs` 8, `svc_encode_slice.rs` 5, `slice_multi_threading.rs` 3,
   `encoder_ext.rs` 2; `grep -rn 'pOverallMbMap' src/encoder`), mechanical
   `AtomicU16`/`AtomicU8`-shaped conversion on the model of T9.C5's `pGomCost`.
   **Verification that it worked**: re-run the mid-row probe under Miri afterwards — it
   still fails, and it must now name the **`&mut SMB` / `uiSliceIdc`** family (round 5,
   session E's), not `SSliceCtx`. Quote the new verdict in the report; if it names
   anything else, that is a fresh finding, not noise. The probes' un-ignore stays E's.
2. **The two deletion rulings are still the user's.** If your kickoff message says to
   take them: F135's twin deletes in step 0 (3 census rows reported as a deletion,
   F128) and **F133's `pGomCost` deletes outright** — the field, the zeroing at
   `rc.rs:792`, the accumulate, *and* the atomics T9.C5 added — with upstream's five
   write-only references quoted. Absent that instruction, follow step 0 as written
   above and leave `pGomCost` atomic.
3. **Two anchors drifted between C's close and this kickoff** — trust these:
   `pRecView` is built in `WelsInitCurrentLayer` at `encoder_ext.rs:~2210` (re-grep
   `pRecView = `), and `WelsIMbChromaEncode`'s two idct sites are at
   `svc_encode_slice.rs:1855` and `:1872` (its slot hoist at `:1844`).

One inherited duty restated because it is easy to drop: **F117's three source-picture
copies are open and nobody owns them after you** — if you have budget after step 6,
take the B4-handover design (record indices in-loop, copy after the join, the `bg`
preset's 48 rows as the proof); if not, say in the report that F117 passes to the
next owner unstarted, so the charter can assign it rather than lose it.

