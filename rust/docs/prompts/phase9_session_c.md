# Phase 9 — Session C: the reconstruction write — the seam, its probes, and the blocked thirty-two

*Self-contained. Read top to bottom once; then work the steps in order. Every count
below was measured at the commit this brief landed in, with the command beside it —
re-run before quoting, trust the tree over this document. Briefs in this phase are
reliably wrong about something structural; find this one's defect and say so plainly.
Your findings start at **F130**. This is the phase's hardest session and it is allowed
to be two (C/C2): the drop order at the end says what carries.*

## What this project is, in one paragraph

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264 (the C++ is
at the repo root, `codec/`). It ships as a drop-in `libopenh264` replacement and must
stay **byte-identical** to the C++ on every stream the gates run. Phase 9 is the
encoder's safety endgame: every file carries `#![deny(unsafe_code)]`, each raw site
has a tag, and the phase retires them family by family. The plan is
`rust/docs/safety_refactor_plan.md` (rules §7.6, cited as S-numbers); the charter is
`rust/docs/prompts/phase9.md`; findings are `rust/docs/phase9_findings.md`.

## What session C is about — and the decision it executes

Every session so far converted **readers** of pictures. C converts the **writers**: the
reconstruction picture — written macroblock by macroblock during encode, read back as
the next frame's reference — plus the deblocking filter that smooths it, in both the
single-threaded and the multi-threaded encoder.

The hard part is multi-threading, and it was measured before it was decided (F107):

- Each MT worker encodes one slice and writes **its own macroblocks'** bytes into the
  one shared reconstruction picture; a worker reads back only what it wrote. The byte
  sets are disjoint. There is no race.
- But **no `&mut` can say so**: a macroblock's bytes are sixteen 16-byte stripes across
  full-width rows, and **three of the four MT slice modes put slice boundaries
  mid-row**, so per-worker `&mut [u8]` bands, `&mut SPicture` per worker (F73), and
  per-MB `&mut` cursors across the fork all fail the same way.

**Decision D-mt-3 (steward 2026-08-22; confirmed by the user 2026-08-23, option A
kept):** the plane is written through **one shared interior-mutable view** built
before the fork from the picture's exclusive borrow, carrying **one tagged
`unsafe impl Sync`** — a new lawful category, `// unsafe-cat: recon-seam` — with all
consumers safe, exactly as D-mt-1 did for the fork's `Send`. The promise is *checked*:
Miri's data-race detector runs the MT encode probe (today `#[cfg_attr(miri, ignore)]`)
plus a **new probe forcing a mid-row slice boundary**. Those two probes passing is
this session's acceptance — not a nice-to-have, the acceptance.

**Not this session**: the `SvcMdSCDMbEnc` blocked rows (3 copy sites —
`SCREEN_CONTENT(dormant)`, Phase 10's; the census still lists them, F128 says why);
the 7 ME-blocked cost sites and the transitional raw cost tables (session F's);
`*mut SDqLayer`/`SSlice`/ctx parameters (E, G–H); the preprocess site.

## Rules that never bend

- **Byte-identical every commit**: `bash rust/tools/gates.sh commit` (~2.5 min) before
  each; `gates.sh family` (sweeps **583 rows** per profile since B4) after risky ones.
  The seam writes the same bytes through a different type — a moved byte is a defect.
- **Session close**: `MIRI_SCOPE=encoder bash rust/tools/gates.sh session` (D-gate-4;
  Miri was 978 s at B4's close and **will grow when the MT probes un-ignore** — budget
  for it, and if the growth is large, say by how much). Code after the close re-runs
  it; docs-only after takes the cheap corroboration, said in those words.
- **No benches, no perf, no unscoped Miri** (D-gate-1/2). **Ratchet only down**; new
  raw tagged same-day (D-exit-1) — *the seam's `unsafe impl Sync` and any
  `UnsafeCell`-crossing accessor are tagged `recon-seam` from the first commit*.
- **S20 closures**, and the slot rule (F118): operand and slot move together; a slot
  **constant after init** may be bypassed by direct call per site, byte-identically —
  `WelsInitEncodingFuncs` installs the `_c` kernels unconditionally, so it applies to
  the copy and inverse-DCT tables here exactly as it did to SAD.
- **Planted-fault calibration per new conversion shape** (S55/S59): the first
  seam-written IDCT, the first copy, the first deblock edge — plant one sample, watch
  a row fail, revert, note the char. And count coverage in *covered rows*: S59 — a row
  that swallows the fault referees nothing at that site.
- **Stay in lane**; blockers become findings.

## Stacked-Borrows and threading facts for this session

1. **A `&mut T` parameter retags all of `T`** (F66); **a `&T` argument is a protector**
   (F114b/S56); **`addr_of_mut!` under a `&mut` parent dies to a sibling safe borrow**
   (F114a/S29) — derive at the call, never hold across.
2. **`SPicture::planes()` is `&mut self`** — repeated calls are a stack, not siblings
   (S40/F73). The seam exists to make that stop mattering: after it, nothing inside
   the frame loop takes `&mut` on the reconstruction picture at all.
3. **The seam's soundness argument, stated once**: the view is built from
   `&mut PaddedPlane`, so for the view's lifetime nothing else may borrow the plane;
   writes go through `UnsafeCell` with no synchronization, which is sound **iff** the
   byte sets are disjoint per worker (F107's measurement) — that "iff" is exactly what
   the two Miri data-race probes check; the fork's join publishes the writes to
   post-join readers (scope join is a happens-before edge). Write that paragraph, in
   the type's doc comment, and keep it honest.
4. **The one stability requirement (F109's shape)**: while the view lives, nothing may
   take `&mut` to the same picture through the pool (`pic_mut(idDec)`,
   `layer_dec_pic_mut`) — that retag would invalidate the view's provenance. Today
   there are **18** `layer_dec_pic`/`_mut` uses (`grep -rn 'layer_dec_pic'
   src/encoder | grep -v ':\s*//' | grep -v 'pub unsafe fn'`); every in-frame one must
   become a view route or be proven pre-fork/post-join. List them in the report.
5. **Deblocking reads across macroblock edges** — the left and top *neighbours'*
   pixels, which under MT can belong to another worker's slice. The C++ is race-free
   here only because slice-boundary edges are skipped in the parallel path
   (`uiFilterIdc`, slice-scoped `uiNeighborAvail` — session B verified "a worker reads
   only what it wrote" *including* deblocking, F107 §2). Do not re-derive this from
   hope: the mid-row probe is built to catch exactly a mistake here.

## The map, measured at this brief's commit

### The seam's consumers — 32 census-blocked call sites

`python3 rust/tools/phase9_plane_callers.py --sites | grep blocked`, by owner:

| owner | sites | groups |
|---|---:|---|
| `WelsRecPskip` | 6 | copy (ref→rec P_SKIP reconstruction) |
| `WelsEncRecI16x16Y` | 5 | 4 idct + 1 copy |
| `WelsMdInterEncode` | 3 | copy |
| `WelsMdBackgroundMbEnc` | 3 | copy (BGD P_SKIP — **lit by `bg`**, 32-row teeth per F126) |
| `OutputPMbWithoutConstructCsRsNoCopy` | 3 | idct |
| `WelsIMbChromaEncode` | 2 | idct |
| `WelsEncRecI4x4Y` | 2 | 1 idct + 1 copy |
| `WelsMdIntraChroma`, `WelsMdI4x4Fast`, `WelsMdI4x4`, `WelsMdI16x16` | 5 | intrapred (the `pRef` operand is the recon picture) |
| `SvcMdSCDMbEnc` | 3 | copy — **dormant, not yours** |

**29 are yours.** The tables they feed: `pfIDctT4`/`pfIDctFourT4`/`pfIDctI16x16Dc`
(`PIDctFunc = unsafe extern "C" fn(*mut u8, i32, *mut u8, i32, *mut i16)`,
`svc_encode_mb.rs:309`), the eight `pfCopy*` slots (`wels_func_ptr_def.rs:363–370`),
and the three intra-pred arrays (`pfGetLumaI16x16Pred`/`pfGetLumaI4x4Pred`/
`pfGetIChromaPred`, `PGetIntraPredFunc` at `wels_func_ptr_def.rs:51`). All constant
after init — direct-call per site first, table flips last (F118).

**The safe kernels already exist** — this session writes no pixel math:
`idct_t4_rec`/`idct_four_t4_rec`/`idct_rec_i16x16_dc` (+ `_in_place` forms,
`encoder/decode_mb_aux.rs:119–219`), `copy_4x4`/`copy_8x8`/… (`common/copy_mb.rs:56+`),
and the intra-pred kernels (`get_intra_predictor.rs:272+`, pred-side over arrays; the
decoder's table of safe `fn` pointers is the model). What they lack is a
**write-through-`&self` flavour** for the destination — the seam view cannot hand out
`&mut` rows, so the rec-writing kernels gain variants taking the view's cursor
(`row_mut` cannot exist on a shared view; a `set_row`/`write_row` on the view can).

### The carrier and the roots

- `SPicData.pCsMb` **26** reads / `pDecMb` **9** (`grep -rn 'SPicData\.pCsMb'
  src/encoder | grep -v ':\s*//' | wc -l`). The carrier already holds `iMbX`/`iMbY`
  (T9.B30) — the recon halves convert to coordinates + the view route, then the two
  raw arrays delete. The stamps live in `WelsMdIntraInit` (`svc_base_layer_md.rs:355`,
  walking cursors from `pCsData`/`pDecPic` roots).
- Roots: `pCsData` **21** reads; `layer_dec_pic`/`_mut` **18**; `planes()` **30**
  calls in `src/encoder` (many are pre-frame stamping — classify before touching);
  the per-frame stamp is `encoder_ext.rs:2183–2188` (`pDecPic = (*pRefList)
  .pic_mut(idDec).planes()`), which is where the *view* gets built instead.
- Deblocking: `encoder/deblocking.rs` — 26 unsafe items, 21 tags;
  `common/deblocking_common.rs` — 19 unsafe fns, un-denied. `SDeblockingFilter`
  carries `pCsData` roots walked per MB; it runs **inside the fork** under MT (F108 —
  the frame-level filter runs only when `!bDeblockingParallelFlag`).

### The fork, and where the view lives

The spawn seam is `slice_multi_threading.rs`: `SliceJobHandle` (`:1193`, carries the
raw ctx; `unsafe impl Send` at `:1251`, D-mt-1), workers run `EncodeOneSliceInJob`
(`:1334`), forks at `std::thread::scope` (`:1443`, `:1511`). The view is built where
the roots are stamped today (pre-fork, per frame) and must be reachable from both the
ST path and the workers. Two placements work; pick one and defend it in the commit:

- **On the layer** (`SDqLayer` gains the view the way it gained `pEncPic` in T9.B21) —
  the natural route, since every consumer already reaches the layer; the view then
  holds raw parts internally (pointer + geometry captured once from the
  `&mut PaddedPlane` at build; every later access derives from that captured root —
  S40's retag-stable shape) rather than a lifetime, because `SDqLayer` is not
  lifetime-parameterized.
- **Through the job** (each `SliceJobHandle` carries a `&` to a scope-local view) —
  lifetimes work here, but the ST path needs its own construction and the layer-based
  consumers need the route threaded; more signatures move.

Whichever you pick: **one** `unsafe impl Sync`, **one** place raw parts are captured,
both tagged `recon-seam`, and the doc paragraph from fact 3 above sits on the type.

### F117 — the three source-picture copies (lit, refereed, and yours to *move*)

`VaaBackgroundMbDataUpdate` (`svc_mode_decision.rs:~510`) copies the previous source
picture into the **current** source picture at background MBs, per MB, in-fork — the
picture every `pEncMb` cursor reads. B4 lit it (`bg` preset; a planted fault fails all
three clips) and measured the constraint: **the source pool cannot become a shared
borrow across the loop while this write is raw; the conversion moves the write, not
borrows around it.**

The design to try: the copy's effect is invisible to the *current* frame — every
in-frame source read is MB-local to its own macroblock, and the copy writes only its
own macroblock — so the copies can be **recorded during the loop (MB index list) and
executed after the join** from plain `&`/`&mut` borrows of the two pictures. That is a
*behavioural reordering*, legal only if truly unread in-frame, and the referee is
already built: the `bg` preset encodes 72–80 frames, so any next-frame divergence is a
failing row. Prove it (all 48 rows, both profiles), and re-run the planted fault after
the move to show the referee still has teeth (S59). If the proof fails — some in-frame
read does see the copied bytes — write down which, leave the three sites raw and
tagged, and hand the fact to the findings; that outcome is legitimate.

## Steps

0. **The seam type + its own probes** (one commit, no consumer changes). The type,
   its doc-paragraph soundness argument, the `recon-seam` tags, and **type-level Miri
   probes**: two scoped threads writing disjoint stripes through one view (passes),
   and — behind `#[should_panic]`-style expectation or a doc note, not left silent —
   the demonstration that the *detector* sees an overlap (a deliberate overlapping
   write that Miri flags, kept as a commented calibration recipe, not a live test).
   Accept: `cargo +nightly miri test --lib -- <the new probes>` green; ratchet flat
   (new tags declared).
1. **The view route** (layer or job — the decision above), built at
   `encoder_ext.rs:~2180` where `planes()` is called today. ST and MT both construct
   it. Accept: byte-identical; nothing consumes it yet.
2. **The consumers, function by function** (the table above; several commits).
   Per function: recon destination through the view's cursor, prediction/coefficient
   operands as B2–B4 did (owned arena buffers, `blk4x4`-style borrows), direct-call
   the constant kernels (F118), re-derive surviving raws after safe borrows end
   (F114a), `q1c.py --kind ref` before and after, planted fault once per new shape.
   `SPicData.pCsMb`/`pDecMb` reads become coordinates + view; delete each raw array
   when its last reader goes.
3. **The tables flip**: `PIDctFunc` → safe over the view-cursor + `&[i16; N]` (three
   slots, two spans — count them, S52/F113); `PCopyFunc` → safe (eight slots); the
   three intra-pred arrays → the decoder's model (`Option<fn(...)>` over safe types).
   Shims delete; `common/copy_mb.rs` and `common/intra_pred_common.rs` reach
   `#![deny(unsafe_code)]`; `get_intra_predictor.rs`'s shim tags drop.
4. **Deblocking**: `SDeblockingFilter`'s roots become the view; the per-MB walk uses
   view cursors; `deblocking_common.rs`'s kernels gain the `&self`-write flavour;
   both files reach `deny` (fact 5's caution governs — the slice-boundary skip is
   load-bearing, and the mid-row probe is its referee).
5. **The two MT Miri probes — the acceptance.** Un-ignore
   `fork_join_encodes_a_multi_slice_frame_under_the_aliasing_checker`
   (`svc_encode_slice.rs:4245`) and the `slice_multi_threading.rs:1124` ignore if the
   seam retires its reason; **add the mid-row probe** (`SM_RASTER_SLICE` uneven rows,
   or `SM_SIZELIMITED_SLICE` at threads=2 — whatever provably puts a slice boundary
   mid-row; assert it did, don't assume). Run them standalone the day they compile —
   not first at the close. Accept: both pass under Miri; the close's Miri includes
   them.
6. **F117's move** (the design above), with its proof or its refusal recorded.
7. **Drop order if this becomes C/C2**: drop 6, then 4, then the tail of 2–3 —
   **never 0, 1, or 5**: a seam without probes is an assertion, and probes without the
   seam test nothing. If you split, close C with the seam + probes + at least
   `WelsRecPskip` converted (the biggest single consumer), and leave a C2 map.
8. **Close**: the session gate (state the Miri wall-time delta the probes added);
   regenerate both censuses; findings from **F130**; log; charter row; tags and
   ratchet re-measured, conversions and reclassifications never summed (F128).

## What to report back

Plain prose: commits with ratchet deltas; every gate verdict including the two probes'
first green runs and the close's Miri time; the seam's final shape (placement, the one
`Sync`, the accessor set) and the soundness paragraph as committed; the consumer table
re-measured (blocked 32 → what remains, by owner); which of the 18 `layer_dec_pic*`
uses survived and why; F117's outcome (moved + proven, or refused + why); where this
brief was wrong, quoting the sentence; and what F, E, and Phase 10 inherit after you.
