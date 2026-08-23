# Phase 9 — Session B2: the plane family, part 1b — source and reference become *names*, and the first tables flip

*Self-contained. Read top to bottom once; then work the steps in order. Every count
below was measured at commit `2f5a2834` (the session's start) with the command shown
beside it — re-run the command before you quote a number, and trust the tree over this
document when they disagree. Two of the last four briefs were wrong about something
structural (F101, F110); assume this one is too, find it, and say so plainly.*

## What this project is, in one paragraph

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264 (the C++ is
at the repo root, `codec/`). It ships as a drop-in `libopenh264` replacement and must
stay **byte-identical** to the C++ on every stream the gates run — that is the referee
for everything you do. The port started as C-shaped Rust full of raw pointers; Phase 9
is the safety endgame for the encoder: make the code safe so the ~700 individually
tagged `#[allow(unsafe_code)]` items can be deleted. The encoder's 39 files already
carry `#![deny(unsafe_code)]`; every remaining raw site has a comment tag
(`// unsafe-cat: port-raw(Phase 9)` or `cursor`) and the phase retires them family by
family. You are one session in that sequence. The plan is
`rust/docs/safety_refactor_plan.md` (its standing rules are §7.6, cited below as
S-numbers); the phase's charter is `rust/docs/prompts/phase9.md`; findings are
`rust/docs/phase9_findings.md` (F101–F114 so far; yours start at **F115**).

## What session B2 is about

The encoder reaches picture pixels three ways: through **dispatch tables of raw
function pointers** (SAD/SATD cost, motion compensation, DCT, copy, intra-prediction);
through a **per-macroblock bundle of raw cursors**, `SPicData`, stamped once per
macroblock from per-frame roots; and through **whole-picture raw roots** handed out by
`SPicture::planes()`. Session B measured all of it by *caller* (`phase9_plane_census.md`)
and found the thing that re-ordered this phase (F104): every cost-table call site pairs
a picture operand with an `SMbCache` prediction buffer, and a table has one type, so no
table could flip until the `SMbCache` family was safe. Session D made it safe (every
`*mut SMbCache` parameter is `&mut SMbCache`; the arena's buffers are plain owned
arrays). **The double gate now has one gate left — the picture half — and that is this
session.**

Concretely, B2 does two things, in this order:

1. **The source and reference pictures become names.** `SPicData`'s `pEncMb` (source)
   and `pRefMb` (reference) cursors, the motion-estimation struct's `pEncMb` /
   `pRefMb` / `pColoRefMb`, and the layer's per-frame roots they are stamped from
   become picture *handles* plus *coordinates*, and every reader builds a bounds-checked
   `PlaneCursor` at the point of use — the decoder's model, already proven there.
2. **The tables whose operands are all safe after (1) flip to safe function pointers,
   one commit each**: `pfSampleSad` / `pfSampleSatd` / `pfSample4Sad`, the motion
   compensation entry points, and `pfDctT4` / `pfDctFourT4`. Their shims go;
   `common/sad_common.rs`, `common/mc.rs` and `encoder/sample.rs` reach
   `#![deny(unsafe_code)]`.

**Not this session**: anything that reads or writes the *reconstruction* picture
(`SPicData.pCsMb` / `pDecMb`, the inverse-DCT table `pfIDctT4` / `pfIDctFourT4`, the
copy table `pfCopy*`, the intra-prediction tables' *reference* side, deblocking, the
multi-threaded fork) — that is session C's, under decision D-mt-3. Converting
`*mut SDqLayer` / `*mut SSlice` / `*mut SMB` parameters — session E's. The context
parameter — sessions G–H. Screen-content code (`SCREEN_CONTENT(dormant)` tags) — Phase
10's. You will *touch* the layer (one new field) and read through raw context pointers
constantly; you do not convert them.

## Rules that never bend

- **Every commit is byte-identical.** These are safety conversions with no intended
  behaviour change. `bash rust/tools/gates.sh commit` before every commit (~2.5 min:
  builds all targets, `cargo test` debug + release, the unsafe ratchet). A moved byte is
  a defect — bisect, do not explain it away.
- **Session close is one command**:
  `MIRI_SCOPE=encoder bash rust/tools/gates.sh session` — the sweeps in both profiles
  (535 rows each) **plus the encoder-scoped Miri `--lib` step**, no benches. It takes
  **≈18 minutes** here (Miri is 894 s of it) and it is not optional: session D passed
  the byte gates at every one of its ten commits and this Miri step then found **two**
  undefined-behaviour defects the session had introduced (F114). That is decision
  D-gate-4. Run the `family` level (sweeps, no Miri, ~2.5 min) after any commit you
  consider risky — D ran it five times.
- **No benches, no perf measurement, no unscoped Miri, no differential Miri** — those
  run at the phase close (D-gate-1, D-gate-2).
- **The ratchet only goes down** (`rust/tools/unsafe_ratchet.sh`; the gate runs it). Any
  *new* raw signature is tagged `// unsafe-cat: port-raw(Phase 9)` the day it is written
  (D-exit-1). A rebaseline needs the reason in the same commit.
- **Commit unit = a signature-reachability closure (S20)**: when you change a function's
  parameter type, every caller and every callee that type reaches changes in the same
  commit, and the tree is green in between. Commit messages carry `T9.B2<n>` and say
  what moved and what was measured.
- **Stay in lane** (the list above). If something outside it blocks you, write the
  blocker down — a finding — and route around it; do not widen.

## Background you need

### Why the order is what it is (two sentences)

A `&mut T` function parameter **retags the whole `T`** at function entry under Stacked
Borrows, so any raw cursor derived from `T` that a caller still holds afterwards is dead
(F66). That is why Phase 9 converts the *cursors* first and the *containers* they point
into last — and why the tables, whose one slot type must serve every caller, flip only
when **both** operand families of every call site are safe (F104, S52). Coefficients
(session A) and `SMbCache` (session D) are done; pictures are yours.

### The safe types (already built, already Miri-tested)

In `src/safe/plane.rs`:
- `PaddedPlane` owns one plane's bytes and knows width, height, stride, padding and
  where the visible origin is. `plane.cursor(x, y) -> PlaneCursor<'_>` anchors a read
  cursor at *logical* sample `(x, y)` — negative and out-of-picture coordinates into the
  padding are legal, which is what motion search needs. `plane.cursor_mut(x, y)` is the
  write form. `plane.stride()`.
- `PlaneCursor<'a>` is **`Copy`**: `at(dx, dy)`, `row(dy, dx0, len)`, `advance(dx, dy)`
  (returns a rebased copy — the safe form of `p = p.offset(..)`), `stride()`. It can
  also be built over **any byte slice**: `PlaneCursor::new(&buf[..], center, stride)` —
  that is how an `SMbCache` prediction buffer (`sMemPredMb`, stride 16; chroma halves,
  stride 8) becomes a kernel operand. It panics only if the anchor is outside the slice.
- `PlaneCursorMut<'a>`: the write version (`set`, `row_mut`, `reborrow(dx, dy)`,
  `advance`, `as_ref()`), and `PlaneCursorMut::new(&mut buf[..], center, stride)`.

In `src/encoder/picture.rs`: `SPicture { planes: [PaddedPlane; 3], .. }` with
`plane(i) -> &PaddedPlane` and `plane_mut(i)`; two pools — the **source** pictures
(`SrcPicPool`, owned by the preprocessor as `m_pSpatialPicPool`, ids `SrcPicId`) and the
**reconstruction/reference** pictures (`RecPicPool`, owned by each layer's `SRefList`, ids
`RecPicId`); `PicRef` is the enum naming either. Resolution helpers that already exist
(`svc_encode_slice.rs:865–1000`): `ctx_pic_ref(pCtx, r) -> Option<&SPicture>` (shared)
and `ctx_pic_ref_mut` (exclusive — **do not use it inside the macroblock loop**, see
below); `layer_ref_pic(pLayer) -> Option<&SPicture>` — the reference picture through
the layer's `pRefList` + `pRefPic: Option<RecPicId>`; `layer_dec_pic` / `_mut` — the
reconstruction picture (C's). The current frame's source id is
`(*pCtx).pEncPic: Option<SrcPicId>` (`encoder_context.rs:1055`).

The **decoder did exactly this conversion**: per-macroblock code reaches pixels as
`plane_mut(i).cursor_mut(x, y)` with sample coordinates, and its prediction kernels are
safe functions stored in plain arrays of safe `fn` pointers — no `unsafe`, no
`extern "C"`. When in doubt about shape, read `src/decoder/decode_slice.rs` (search
`cursor_mut(`) and copy.

The kernels under the shims are **already safe**:
`sample_sad::<W, H>(&PlaneCursor, &PlaneCursor) -> i32` and
`sample_sad_four::<W, H>(.., &mut [i32; 4])` (`common/sad_common.rs:468/485`);
`satd_4x4` … (`encoder/sample.rs:47–109`); `mc_luma(src: &PlaneCursor, dst: &mut
PlaneCursorMut, mv_x, mv_y, w, h)`, `mc_chroma`, `mc_hor_ver02/20/22`, `pixel_avg`
(`common/mc.rs:741–1041, 367–433, 346`); `dct_4x4(&mut [i16; 16], &PlaneCursor,
&PlaneCursor)` and `dct_four_4x4(&mut [i16; 64], ..)` (`encoder/encode_mb_aux.rs:331/379`).
What is `unsafe` is the shim around each (`WelsSampleSad16x16_c(*mut u8, i32, *mut u8,
i32)` and friends) and the raw slot types that hold them. Delete a shim by fixing its
callers and re-typing its slot; the kernel stays.

### Five Stacked-Borrows facts this session must keep in its head

You will be told "byte-identical" by every gate and still be wrong; only the Miri step
sees this class, and it runs once, at your close. So reason first:

1. **A `&mut T` parameter retags all of `T` on entry** (F66). A raw cursor into `T`
   held by the caller across the call is dead afterwards.
2. **A shared `&T` argument is a *protector* for the whole call** (F114b, S56). Nothing
   may invalidate it while the call runs — not even transiently, not even if the callee
   only reads. `AddSliceBoundary` took `&SMB` "because it only reads" and a callee three
   levels down re-borrowed the same macroblock mutably: undefined behaviour that the
   raw pointer never had. Before you convert a parameter, walk the callee's callees.
3. **`addr_of_mut!` stops rescuing a derivation the moment its parent stops being raw**
   (F114a, S29's clause). While `p` is `*mut T`, `addr_of_mut!((*p).field)` reuses the
   parent's item and a sibling borrow does not pop it. Once `p` is `&mut T`, that same
   spelling gets its own item *above* the parent's `Unique`, and any sibling safe borrow
   of the field kills it. **When a parameter's type changes, every derivation beneath it
   changes meaning — re-audit them all.** That is what cost session D two defects.
4. **An accessor that hands out a cursor must be retag-stable** (S40). `&mut self.buf`
   is a `Unique` over the whole allocation, so calling such an accessor twice pops the
   first cursor. `PaddedPlane::root_ptr` reads the address without forming a reference,
   which is why repeated calls are siblings. **`SPicture::planes()` takes `&mut self`**
   — every call is a fresh exclusive borrow of the whole picture. Calling it per
   macroblock is F73's retag, and two of the sites you will touch do exactly that
   (`svc_mode_decision.rs:1842`, `:1899`: `ctx_pic_ref_mut(..).map(|p| p.planes())`
   inside the macroblock loop). **Inside the loop, resolve pictures through the *shared*
   route** (`ctx_pic_ref`, `layer_ref_pic`) and build cursors from `&SPicture`; a shared
   borrow handed out twice is two siblings, not a stack.
5. **A byte gate cannot see any of this.** Session D shipped ten commits at 535/535 in
   both profiles with two live UB in them. Your sweeps prove you did not change the
   output; only the Miri step at your close proves you did not break the aliasing.

## The map, measured at `2f5a2834`

Every number below has its command beside it. Re-run it; the tree wins.

### (1) The per-macroblock carrier `SPicData`

`grep -rn 'SPicData\.' src/encoder | grep -v ':\s*//' | wc -l` → **106** uses.

| field | surface | uses | this session? |
|---|---|---:|---|
| `pEncMb` | source picture at this macroblock | 45 | **yes** |
| `pRefMb` | reference picture at this macroblock | 26 | **yes** |
| `pCsMb` | reconstruction ("current slice" view) | 26 | no — session C |
| `pDecMb` | reconstruction | 9 | no — session C |

By file: `svc_base_layer_md.rs` 60, `svc_mode_decision.rs` 32, `svc_encode_slice.rs` 9,
`svc_encode_mb.rs` 5. The struct is `encoder_context.rs:157`, four `[*mut u8; 3]`
arrays, and it lives inside `SMbCache` (`md.rs:491`), which session D made a safe
`&mut SMbCache` at every parameter.

**Where it is stamped** — two places, and they behave differently:

- **Source and reconstruction**, in `WelsMdIntraInit` (`svc_base_layer_md.rs:313`):
  at the first macroblock of a row it computes
  `(*pCurLayer).pEncData[i].offset((mbX + mbY*stride) << 4)` (chroma `<< 3`), and for
  every following macroblock it *advances* the existing pointer by 16 / 8. So the
  bundle is a **walking cursor**, not a fresh derivation per macroblock.
- **Reference**, in `WelsMdInterInit` (`svc_base_layer_md.rs:~937`): the same shape,
  from `(*pCurLayer).sRefPicView.sPlanes.pData[i]`, re-anchored when
  `0 == kiMbX || iSliceFirstMbXY == kiMbXY` and advanced otherwise.

The replacement is the decoder's: **the carrier holds names and coordinates**, and each
reader builds its own cursor at the point of use. The walking arithmetic disappears —
`(mb_x, mb_y)` is already in `SMB` (`iMbX`, `iMbY`), so a reader computes
`pic.plane(0).cursor(16*mb_x + dx, 16*mb_y + dy)` and the padding rules are the
`PaddedPlane`'s, not the caller's.

### (2) The motion-estimation cursors

`SWelsME` (`svc_motion_estimate.rs:153`) carries `pEncMb`, `pRefMb`, `pColoRefMb`
(`:164–166`) — the source block, the reference block the search walks, and the
co-located reference block. `grep -rn 'pEncMb\b' src/encoder | grep -v 'SPicData.pEncMb'
| wc -l` → **53** uses (20 in `svc_motion_estimate.rs`).

They are set in exactly four places: `InitMe` (`svc_mode_decision.rs:1208`, the entry
point — `pEnc`/`pRef` are parameters), and `svc_motion_estimate.rs:406`, `:659`, `:926`,
each of which **re-points `pRefMb` at a candidate position** (`:659` is
`pColoRefMb.offset((mv_y * stride + mv_x))`). That is what a `PlaneCursor` is for:
`advance(dx, dy)` returns a rebased copy, and the cursor may leave the visible picture —
the plane's padding is addressable, which is precisely what a search needs.

`q1c.py --type SWelsME --kind ref` → **0 hazards across 20 bodies**: the struct is
already `&mut SWelsME` everywhere, so this is a field-type change inside a type that is
already a reference, not a parameter conversion.

### (3) The tables that flip, and what they cost

| table | slot type | mentions | shims to delete | file reaching `deny` |
|---|---|---:|---|---|
| `pfSampleSad` | `PSampleSadSatdCostFunc` (`md.rs:254`) | 30 | 14 `WelsSampleSad*_c` | `common/sad_common.rs` |
| `pfSampleSatd` | same type | 19 | 7 `WelsSampleSatd*_c` | `encoder/sample.rs` |
| `pfSample4Sad` | `PSample4SadCostFunc` | 11 | (in `sad_common.rs`) | — |
| `pfDctT4` / `pfDctFourT4` | `PDctFunc` (`encode_mb_aux.rs:203`) | — | `WelsDctT4_c`, `WelsDctFourT4_c` | — |
| motion compensation | **not a slot** — direct calls | 33 | 22 dead `mc.rs` shims (F105) | `common/mc.rs` |

Commands: `grep -rn 'pfSampleSad\b' src/encoder src/processing | grep -v ':\s*//' | wc -l`
(and the two siblings); `grep -c 'unsafe fn\|unsafe extern "C" fn' src/common/sad_common.rs`.

Three facts about these tables that decide how the commits are shaped:

- **`pfSampleSad` and `pfSampleSatd` share one type, and one more thing reads them.**
  `SSampleDealingFunc::md_cost(block)` and `me_cost(block)` (`md.rs:~755`, `:770`)
  select between the two arrays by a `CostFamily` enum. They are the read path for most
  call sites, and they return `Option<PSampleSadSatdCostFunc>` — so the return type
  changes with the slot type, in the same commit.
- **42 sites pass or store a cost slot as a value** rather than calling it —
  `python3 rust/tools/phase9_plane_callers.py --handoff`. `pfCalculateSatd` takes one as
  its first argument (`svc_motion_estimate.rs:587`, `:632`, `:676`), the four search
  entry points hoist one into a local, and `SFeatureSearchIn::pSad` **stores** one
  (`:1400`). Re-typing a slot re-types all 42 signatures. They carry no operands, so
  they are invisible in the call-site census — S20 says they are in the closure.
- **Motion compensation is not dispatched.** Phase 4a made `McLuma_c` / `McChroma_c`
  direct calls (33 sites: `svc_base_layer_md.rs` 17, `md.rs` 10, `svc_mode_decision.rs`
  6), and `md.rs` additionally calls `McHorVer02_c` / `McHorVer20_c` / `McHorVer22_c` /
  `PixelAvg_c` directly in the ME refinement. So MC needs **no slot re-typing at all** —
  it is a caller conversion to `mc_luma` / `mc_chroma` / `mc_hor_ver*` / `pixel_avg`,
  which already exist, are safe, and are what the decoder calls.

### (4) The layer roots the carrier is stamped from

`SDqLayer` (`svc_encode_slice.rs:1014+`) holds `pEncData: [*mut u8; 3]` (source),
`pCsData: [*mut u8; 3]` (reconstruction — C's), `sRefPicView: SRefPicView` (a
**by-value copy** of the reference picture's `planes()` output plus its type), and the
handles `pRefPic: Option<RecPicId>`, `pDecPic: Option<RecPicId>`,
`pRefOri: [Option<PicRef>; MAX_REF_PIC_COUNT]`.

`pEncData` and `pCsData` are stamped once per frame in `encoder_ext.rs:2151–2172`, from
`(*pCtx).pEncPic: Option<SrcPicId>` and `(*pCtx).pDecPic: Option<RecPicId>` — **the
handles already exist at the stamping site**; they are resolved to `planes()` and the
raw roots are kept. `grep -rn '\.pEncData\b' src/encoder | grep -v ':\s*//' | wc -l` →
**6**; `sRefPicView` → **33**.

**The asymmetry that shapes step 1:** the *reference* picture is reachable from the
layer alone (`layer_ref_pic(pLayer)` resolves `pRefList` + `pRefPic`), but the *source*
picture is not — the layer holds only the raw `pEncData` roots, and the source pool
lives in `(*pCtx).pVpp.m_pSpatialPicPool`. Ten of the consumers take
`pCurDqLayer: *mut SDqLayer` **and no context** (`WelsMdI16x16`, `WelsMdIntraChroma`,
`WelsMdP16x16`, `WelsMdP16x8`, `WelsMdP8x16`, `WelsMdP8x8`, `WelsRecPskip`, and three
dead ones — see the observations). So converting `pEncMb` needs a **source-picture
handle on the layer**, stamped beside `pEncData` in `encoder_ext.rs:2151`, plus a route
from the layer to the pool. That is one new field and one accessor; it is step 1.

## Three things that will bite, found while scoping this session

These are not hypotheticals. Each was measured against the tree at `2f5a2834`.

### (a) The source pool is **not** read-only during the macroblock loop

`phase9_plane_census.md` says `src` is "read-only for the whole frame — any number of
shared `PlaneCursor`s, on any number of threads, is sound." **That is false for one
path, and it is a path this session converts.** `VaaBackgroundMbDataUpdate`
(`svc_mode_decision.rs:510`, called from `WelsMdBackgroundMbEnc` at `:623`, inside the
macroblock loop and in-fork) copies the *current* source macroblock into the
*previous* source picture:

```rust
copy16((*pVaaInfo).pCurY.offset(kiOffsetY), kiPicStride,
       (*pVaaInfo).pRefY.offset(kiOffsetY), kiPicStride);   // dst is pRefY
```

and `pVaaInfo.pRefY/U/V` are stamped from
`self.m_pSpatialPicPool.get_mut(idRef).planes()` (`wels_preprocess.rs:1833`, `:1840`) —
**a different picture in the same pool the source is read from**.

So during one macroblock the encoder holds a read cursor into pool element `idCur` and
writes pool element `idRef`. A single `&SrcPicPool` cannot hand out `&mut` to one
element while shared borrows into another are live, and `get_mut` on the pool would
retag it. Decide this deliberately in step 1 and write down what you chose. Three
options, none free: index-disjoint access on the pool (the pool's own `get2_mut`-style
accessor, if you add one), resolving the two pictures once before the loop and carrying
`(&SPicture, &mut SPicture)` down, or **leaving these three copy sites raw and tagged**
for session C, which already owns a write-under-shared-view problem (D-mt-3). The last
is legitimate and cheap; taking it costs three `port-raw` tags and no soundness.

### (b) Both instruments are stale or blind — repair them before you scope anything

- **`phase9_plane_callers.py` is silently wrong at HEAD.** Its operand classifier keys
  on the ten `md::mem_pred_*` / `skip_mb` / `coeff_level` accessor **names** (lines
  148–176), and session D deleted every one of those accessors in favour of the
  `addr_of_mut!((*pMbCache).sSkipMb)` spelling. Run it today: **40 unclassified, 0
  coefficient-gated**, against session B's 0 and 13. Nothing failed; the totals just
  moved. Adding the field spellings (`sSkipMb`, `sBestPred*`) to the `cache` rule
  recovers 6 sites in one line — the rest are the coefficient spellings. **The tool
  prints its unclassified count and exits 0**; make an unclassified operand loud the way
  S46/S55 made an empty scan loud, because this census is what B2, C and E scope
  against.
- **`q1c.py` cannot see F114's two shapes.** It models shape A (a raw cursor held across
  a retagging call) and shape B (argument evaluation order). Session D's two defects
  were **shape C** — a raw held across a *safe borrow of the same field* — and **shape
  D** — a reference argument protected across a call that re-borrows what it names.
  Both are cheap textual scans, and you are about to create both shapes at scale: every
  `SPicData` field you convert is a derivation whose parent type changed (fact 3), and
  every cursor you turn into a `&PlaneCursor` argument is a protector (fact 2). Add
  them, and run `--type SDqLayer` before you touch the layer: it reports **14 hazardous
  sites in 5 callers** across 77 bodies today.

### (c) `planes()` inside the macroblock loop is F73's retag, already in the tree

`svc_mode_decision.rs:1842` and `:1899` call
`ctx_pic_ref_mut(pEncCtx, r).map(|p| p.planes())` **per macroblock** —
`&mut SPicture` → `planes()` → three raw roots, once per macroblock, on the same
picture. Repeated exclusive borrows of one picture are a stack, not siblings (fact 4).
These are two of the sites you convert; converting them to the shared route
(`ctx_pic_ref`, which exists at `svc_encode_slice.rs:906`) is part of the work, not a
detour.

## Steps

Work them in order. Each is one or more commits; every commit is byte-identical and
carries `T9.B2<n>`.

### Step 0 — Repair both instruments (one commit, no production code)

**Goal:** the census tells the truth at HEAD, and the detector can see the shapes this
session is about to create.
**Do:** (a) fix `phase9_plane_callers.py`'s classifier for the post-session-D spellings
and make a non-zero unclassified count **loud** — a printed warning is not enough if
the exit code is 0; (b) add F114's shape C and shape D to `q1c.py`; (c) regenerate
`phase9_plane_census.md` and commit it with the counts restored.
**Accept:** unclassified is 0 (or every survivor is listed in the doc with a reason);
`q1c.py --type SDqLayer` and `--type SPicture --kind ref` both run and report; the
census's group table again shows the coefficient column populated.
**Note:** if the classifier turns out to need more than the field spellings, say so and
timebox it — a census that is honest about 6 unknowns beats a census that guesses.

### Step 1 — The layer can name its source picture (one commit)

**Goal:** `SDqLayer` gains `pEncPic: Option<SrcPicId>` beside `pEncData`, and a shared
accessor resolves it to `&SPicture` the way `layer_ref_pic` does for the reference.
**Facts:** stamped at `encoder_ext.rs:2151–2160`, where `idEnc` is already in hand; the
pool is `(*pCtx).pVpp.m_pSpatialPicPool`; `wels_preprocess.rs:1482` already has
`src_id(&self, id) -> &SPicture`, the shared form.
**Do:** add the field; stamp it; add `layer_enc_pic(pLayer, pCtx) -> Option<&SPicture>`
(or whatever shape the ten layer-only consumers can actually call — read them first,
they take `*mut SDqLayer` and no context). **Do not delete `pEncData` yet.**
**Accept:** the field is stamped and read by at least one consumer; `pEncData` still
stands; byte-identical.

### Step 2 — The carrier becomes names (two to four commits)

**Goal:** `SPicData` carries `enc: Option<SrcPicId>`, `refp: Option<PicRef>`, `mb_x`,
`mb_y`; every **source** and **reference** reader builds a `PlaneCursor` at the point of
use; the `pEncMb` and `pRefMb` raw arrays are deleted.
**Facts:** 45 + 26 reads; stamped in `WelsMdIntraInit` (`:313`) and `WelsMdInterInit`
(`:~937`) as walking cursors; the reconstruction fields stay exactly as they are.
**Do:** strangler — add the new fields, stamp both old and new, convert readers in
groups small enough to gate, delete each raw array when its last reader is gone.
Convert the two in-loop `planes()` sites (trap (c)) to the shared route as you reach
them. Decide trap (a) here and record the decision in the commit message.
**Accept:** `grep -rn 'SPicData\.\(pEncMb\|pRefMb\)' src/encoder` is 0; `pCsMb` /
`pDecMb` untouched; byte-identical at every commit; ratchet down.

### Step 3 — The ME cursors become cursors (one to two commits)

**Goal:** `SWelsME::pEncMb` / `pRefMb` / `pColoRefMb` become `PlaneCursor<'_>` (or a
handle + offset if the lifetime fights you — say which and why), set through `InitMe`
and rebased with `advance` at `svc_motion_estimate.rs:406`, `:659`, `:926`.
**Facts:** `q1c.py --type SWelsME --kind ref` is 0 across 20 bodies; the struct is
already behind `&mut` everywhere; the search reads outside the visible picture on
purpose — that is the plane's padding and `PlaneCursor` addresses it.
**Accept:** the three raw fields are gone; the search's byte output is identical (the
`st` and `mt` sweep presets cover it); ratchet down.

### Step 4 — The cost tables flip (one commit per table, three commits)

**Goal:** `pfSampleSad`, `pfSampleSatd`, `pfSample4Sad` hold safe function pointers
(`fn(&PlaneCursor, &PlaneCursor) -> i32`, and the four-candidate variant's
`fn(&PlaneCursor, &PlaneCursor, &mut [i32; 4])`); `md_cost` / `me_cost` return the new
type; all 42 handoff signatures follow; the 14 + 7 shims are deleted;
`common/sad_common.rs` and `encoder/sample.rs` carry `#![deny(unsafe_code)]`.
**Facts:** installed in `WelsInitSampleSadFunc` (`sample.rs:332`); the kernels are
`sample_sad::<W, H>` / `sample_sad_four::<W, H>` (`sad_common.rs:468`, `:485`) and
`satd_*` (`sample.rs:47+`); the second operand at 25 of 37 sites is an `SMbCache`
buffer, which is now a plain owned array — build a cursor over it with
`PlaneCursor::new(&cache.sMemPredMb[off..], 0, 16)` (chroma halves: stride 8).
**Accept:** each table's commit is green on `gates.sh commit`; after the third,
`sad_common.rs` and `sample.rs` deny and their tags are gone; byte-identical.

### Step 5 — Motion compensation loses its shims (one to two commits)

**Goal:** the 33 direct `McLuma_c` / `McChroma_c` / `McHorVer*_c` / `PixelAvg_c` calls
become `mc_luma` / `mc_chroma` / `mc_hor_ver*` / `pixel_avg` over cursors; the **22
`mc.rs` shims with no caller outside their own file are deleted** (S18, F105);
`common/mc.rs` carries `#![deny(unsafe_code)]`.
**Facts:** the dead list, by `grep -rn '\bMcHorVer01_c\b' src/encoder src/decoder
src/processing src/api` per name: `McCopyWidthEq{2,4,8,16}_c`, `McCopy_c`,
`HorFilterInput16bit_c`, `FilterInput8bitWithStride_c`, `McHorVer{01,03,10,11,12,13,21,23,30,31,32,33}_c`,
`McHorizLuma_c`, `McVertLuma_c`, `McChromaWithFragMv_c`. Six shims have live callers:
`McLuma_c`, `McChroma_c`, `McHorVer{02,20,22}_c`, `PixelAvg_c` — those are the 33 sites.
**Caution:** `mc.rs` is `common/`, shared with the decoder. The decoder calls the *safe*
kernels (`decode_slice.rs:1084` imports `mc_luma`, `mc_chroma`, …), so deleting shims
should not touch it — **verify with a grep before each deletion, not after** (T9.D8 wrote
three slots off as "decoder-shared" on an unchecked claim and one grep disproved it).
**Accept:** `mc.rs` denies; the deleted names appear nowhere; byte-identical.

### Step 6 — The forward DCT table flips (one commit)

**Goal:** `pfDctT4` / `pfDctFourT4` hold `fn(&mut [i16; 16], &PlaneCursor,
&PlaneCursor)` / `fn(&mut [i16; 64], ..)`; `WelsDctT4_c` and `WelsDctFourT4_c` are
deleted; `WelsDctMb` (`svc_encode_mb.rs:459`) takes safe operands.
**Facts:** the kernels are `dct_4x4` / `dct_four_4x4` (`encode_mb_aux.rs:331`, `:379`);
session D already flipped the other eleven residual slots and the helpers
`blk4x4_mut` / `blk_four4x4_mut` (`:260`, `:267`) are how a sub-block is reached.
**The inverse DCT (`pfIDctT4`, `pfIDctFourT4`) is NOT in scope** — it writes the
reconstruction plane. Leave both, tagged.
**Accept:** the two forward slots are safe; `pfIDct*` untouched; byte-identical.
**Watch fact 3 here specifically**: `WelsDctMb`'s callers derive a raw into
`sCoeffLevel` and the slot you are flipping takes a safe borrow of that same field.
That is exactly F114a's shape. Derive at the call, never hold across.

### Step 7 — If you are running short, drop from the end

Drop in this order: step 6, then step 5, then step 3. **Never drop steps 0–2**: the
instruments and the carrier are what the next session scopes against, and a half-stamped
carrier is worse than none. Say in the report what you dropped and what it costs.

### Step 8 — Close

1. `MIRI_SCOPE=encoder bash rust/tools/gates.sh session` → OVERALL PASS. Miri is not
   optional and it is ~15 minutes; start it before you write the report.
2. Regenerate both censuses (`phase9_census.py --write`, `phase9_plane_callers.py`) and
   commit them — session D closed with the census 49 tags stale.
3. Findings (**F115** onward) into `rust/docs/phase9_findings.md`; the session's record
   into `rust/docs/safety_refactor_log.md`; the charter's §8 row for B2.
4. Report the ratchet deltas and the tag count (`grep -rn 'port-raw(Phase 9)' src | wc -l`
   — **638** at your start).

## Observations to record, not to act on

Two things the scoping turned up that are outside your lane. Write them into the
findings with the evidence; do not fix them here.

- **Three functions are dead in the port and disabled upstream.** `WelsMdP4x4`,
  `WelsMdP8x4`, `WelsMdP4x8` (`svc_base_layer_md.rs:1084`, `:1154`, `:1224`, ~210 lines,
  4 tagged allows, 6 `SPicData` reads between them) have **no caller in the port**, and
  upstream calls them only inside `#if 0 //Disable for sub8x8 modes for now`
  (`svc_mode_decision.cpp:635–655`). They are S18 deletions — and deleting them removes
  6 of the 71 `SPicData` reads you would otherwise convert. Worth doing; not worth
  doing without a decision, because "dead in the port" and "dead upstream" are different
  claims and the second one is the user's call.
- **`SMcFunc` is a dispatch table nobody reads.** `InitMcFunc` (`mc.rs:2219`) fills six
  slots, in **both** codecs (`encoder_context.rs:1586`, `decoder_core.rs:2145`), and
  `grep -rn 'pMcLumaFunc\|pfLumaHalfpelHor\|pfSampleAveraging' src/` finds no read
  anywhere except an `is_none()` assertion. Phase 4a made MC direct and left the table
  standing. It is ABI-pinned (`abi_guard.rs:184`, `assert_size!(SMcFunc, 48)`), which is
  why this needs a decision rather than a deletion.

## What to report back

Plain prose, no ceremony. Cover:

1. **Commits**, one line each: hash, what moved, what it cost (ratchet delta).
2. **The gate verdicts** — every `gates.sh commit`, the `family` runs, and the closing
   `session` run with its Miri tally. If Miri found something, that is the most
   important paragraph in the report: what it was, which commit introduced it, and
   whether the fix removed a pointer or restored one.
3. **The counts, re-measured** — `SPicData` reads remaining, tags, ratchet, and the
   census's four columns.
4. **Where this brief was wrong.** Two of the last four briefs were wrong about
   something structural, and both times the session found it by reading the tree. Quote
   the sentence and say what the tree says instead. Trap (a) above is my reading of the
   VAA write path from three greps — if it is wrong, say so plainly.
5. **What the next session inherits**: what session C needs (the reconstruction half,
   with whatever you learned about pool aliasing), what session E needs (the layer:
   `q1c.py --type SDqLayer` was 14/5 at your start), and anything you left tagged.
