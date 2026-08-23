# Phase 9 — Session A: the map, the detector, and the coefficient proof

You are one session in a long, careful port of the openh264 codec from C/C++ to Rust
(`rust/crates/openh264-rs/`; the C++ reference is under `codec/`). The crate is a
**byte-identical drop-in** for `libopenh264`. Phase 9 is the safety endgame: the encoder
already compiles under `#![deny(unsafe_code)]` on every file, but only because ~760
remaining unsafe sites carry a tagged `#[allow(unsafe_code)]`. Phase 9 retires those tags
by making the code actually safe, so the encoder ends unsafe-free like the decoder already
is. This is the first session. Read `rust/docs/prompts/phase9.md` (the charter) once for the
whole-phase shape; this brief is self-contained for session A.

Session A has three jobs: **(1) build the map** the rest of the phase executes against,
**(2) wire the aliasing detector** the context conversion will need, and **(3) prove the
conversion cadence** on the one family that is genuinely independent of everything else —
the integer-coefficient kernels. Work top to bottom; drop from the end. Commit as
`T9.A1`, `T9.A2`, … Findings go in a new `rust/docs/phase9_findings.md` from **F101**.
Breadcrumbs to `rust/docs/safety_refactor_log.md` as you go.

---

## The core fact that orders the whole phase

Everything below the C-ABI boundary is one connected graph of `unsafe`, and it has a
forced conversion order. The proof, already measured (F66, `phase6_findings.md`):
converting a `*mut sWelsEncCtx` parameter to `&mut sWelsEncCtx` is **UB today** and Miri
rejects it, because a `&mut ctx` function-entry retag invalidates the *entire* 65-field
context under Stacked Borrows — so any caller holding a context-derived cursor across the
call (the port's dominant idiom) is invalidated. That idiom is sound only while the cursor
is a **raw** pointer (a raw retag pops only the offsets it touches). So the raw cursors
must go *first*; then the context conversion becomes legal; then the large tail of
`unsafe fn` whose bodies merely call those things becomes safe automatically.

You are not doing the context conversion this session. You are (a) mapping the graph so
later sessions can, and (b) converting the one family that sits at the very bottom of it —
the coefficient kernels — which needs nothing else converted first.

---

## Hard rules

1. **No behaviour change. Every commit is byte-identical.** These are safety conversions.
   The conformance goldens, the 2919-row malformed corpus, and both benches must produce
   identical bytes and codes before and after every commit. A moved byte is a defect, found
   by bisect across the session's (small) commits — not a new golden.
2. **Reuse the safe vocabulary; invent nothing.** `safe::plane::{PaddedPlane, PlaneCursor,
   PlaneCursorMut}`, `safe::pool::{Pool, Id}`, `safe::mb_grid::MbGrid` already exist and are
   proven. A coefficient block is `&mut [i16; N]` — a plain array slice, no new type.
3. **The ratchet only goes down.** `rust/tools/unsafe_ratchet.sh check` green per commit;
   each conversion *removes* `unsafe fn` / `unsafe {` / `*mut` / `#[allow(unsafe_code)]`
   and their `// unsafe-cat:` tags. If a count must rise for a real reason, rebaseline
   (`unsafe_ratchet.sh generate`) in the same commit with the reason in the message.
4. **Gating (Phase 9 runs sweeps per session, Miri at the phase exit).** Per commit:
   `rust/tools/gates.sh commit`. Per session close: `rust/tools/gates.sh family` (adds the
   `st mt def sl ltr` sweeps in both profiles) — this is your byte-parity insurance.
   **Do not run Miri per commit or per session**; the phase runs it unscoped once, at its
   exit. **No perf work** (D-gate-1): no benches, no spans, this session or any Phase 9
   session but the last.
5. **Do not touch** (owned elsewhere): the `C-ABI` / `C-ABI(test)` tags (`src/api/`, the
   frozen boundary), the `SCREEN_CONTENT(dormant)` tags (Phase 10), the `*mut sWelsEncCtx`
   conversion (a later session, and blocked), the plane / SMbCache / layer families (later
   sessions — see the map you will build). Stay inside the coefficient family for the
   conversion.

---

## Current state — verified facts at `acebd103` (re-grep before quoting; the tree wins)

### The tag queue

```bash
grep -rhn 'unsafe-cat:' rust/crates/openh264-rs/src | sed 's/.*unsafe-cat: //;s/ *$//' | sort | uniq -c | sort -rn
```
gives `port-raw(Phase 9)` **695**, `cursor` **61**, `C-ABI` 45, `MT` 21,
`SCREEN_CONTENT(dormant)` 8, `C-ABI(test)` 5, `send-seam(Phase 9)` 1. The first two (756)
are Phase 9's; the rest are not.

The 695 `port-raw` tags, classified by the signature each sits above (this is the shape
your census tool formalises):

- **419** — `unsafe fn` with a *safe* signature; unsafe only because the **body** derefs a
  raw cursor or calls an unsafe fn. These are **not converted directly** — they go safe when
  their callees do.
- **120** — a `*mut sWelsEncCtx` parameter (the F66 conversion; blocked; a later session).
- **51** — `*mut u8` plane/prediction cursors (the plane family; a later session).
- **33** — `*mut SDqLayer`/`SPicture`/`SSlice` (the layer/picture family; later).
- **31** — other raw signatures (fn-pointer installers etc.; case by case).
- **25** — `*mut i16`/`*const i16` coefficient buffers (**this session's family**).
- **15** — `*mut SMbCache`/`*mut SMB` (the MB-metadata family; later).

### The coefficient family — why it is the clean first proof

The DSP kernels are **already safe**. In `src/encoder/encode_mb_aux.rs`,
`dct_4x4(dct: &mut [i16; 16], pix1: &PlaneCursor, pix2: &PlaneCursor)` and
`quant_4x4(dct: &mut [i16; 16], ff: &[i16; 8], mf: &[i16; 8])` take safe types. The
`unsafe` is a **shim** wrapping each for a raw-pointer caller. Two sub-shapes:

- **Plane-coupled** (`WelsDctT4(pDct: *mut i16, pPixel1: *mut u8, s1, pPixel2, s2)`,
  `encode_mb_aux.rs:~590`) — the shim needs both a coeff buffer and two plane pointers, so
  it converts with the **plane** family, not now. Leave these.
- **Coefficient-only** — the shim takes only `*mut i16`/`*const i16` (no `*mut u8`):
  `WelsIHadamard4x4Dc(pRes: *mut i16)` (`:420`), `WelsDequantLumaDc4x4(pRes: *mut i16, kiQp)`
  (`:435`), `WelsDequantIHadamard2x2Dc(pDct: *mut i16, kuiMF)` (`:448`), the forward
  `WelsHadamardT4Dc`, the `WelsQuant*`/`WelsScan*`/`WelsCalculateSingleCtr`/
  `WelsGetNoneZeroCount` shims. **These are this session's set.** Each does
  `let x: &mut [i16; N] = from_raw_parts_mut(pRes, N).try_into().unwrap();` then calls the
  safe kernel — a **pure round-trip through raw for nothing**.

The callers pass either a **local array** or an **owned inline array**:
```rust
// src/encoder/svc_encode_mb.rs:583  — aDctT4Dc is a local [i16; 16]
WelsIHadamard4x4Dc(aDctT4Dc.as_mut_ptr());
WelsDequantLumaDc4x4(aDctT4Dc.as_mut_ptr(), uiQp as i32);
// :921, :1110, :1126 — aDct2x2 / dc_buf / dct2x2, all locals
```
and `SMbCache` (`src/encoder/md.rs:421`) owns `sCoeffLevel: [i16; MB_COEFF_LIST_SIZE]`
inline — a borrowable array, not a raw pointer. So every input to the coefficient-only
kernels is already an owned `[i16; N]` on the caller side; the raw pointer is a
`as_mut_ptr()`/`from_raw_parts` round-trip that deletes cleanly. There are ~8 call sites in
`svc_encode_mb.rs` and a handful in `svc_base_layer_md.rs`. No context, no plane, no MbCache
*struct* pointer is involved — which is exactly why this family proves the cadence without
depending on anything else.

`src/encoder/decode_mb_aux.rs` re-exports the encoder-side inverse kernels; check whether
its coefficient-only kernels belong to this set too (they share the same round-trip shape).

---

## Steps

### Step 1 — The Phase 9 census (the map every later session needs)
**Goal:** a checked-in, regenerable classification of all 756 Phase-9 tags into families,
with per-file counts and the dependency order, so sessions B–J each know their slice and
the order is not re-derived each time.
**Do:** write `rust/tools/phase9_census.py` that greps the `port-raw(Phase 9)` and `cursor`
tags, reads the signature each sits above, and buckets them into the seven families named
above (body-only, ctx-param, plane, layer/pic/slice, coeff, SMbCache, other) plus the 22
dispatch survivors and the 61 cursor accessors. Emit a table (family × file × count) and
the family dependency edges (coeff and dispatch-survivors are roots; plane/MbCache/layer
feed the ctx conversion; the ctx conversion feeds the 419 body-only). Write the output to
`rust/docs/phase9_census.md`.
**Accept:** `python3 rust/tools/phase9_census.py` reproduces the numbers in this brief
(695/61 split, the seven-family breakdown ±small); the doc names, for each family, the files
it lives in and what it waits on. This is the artefact session B reads first.

### Step 2 — Wire the aliasing-hazard detector (F66's precondition)
**Goal:** the instrument the `*mut ctx` conversion (a later session) must run *before* it
converts, per F66, exists and is documented now — not rebuilt under pressure later.
**Facts:** session J built `q1c.py` (reproduced in its log entry, `safety_refactor_log.md`
— search "q1c") to find call sites that hold a context-derived pointer across a
`&mut ctx`-style call. It found the 93 hazardous sites in 28 callers that blocked the
conversion.
**Do:** recover `q1c.py` from the session-J log into `rust/tools/q1c.py`, confirm it runs
against the current tree, and document at its top: what it detects, why (the F66 retag), and
that a later session runs it as the **precondition** to the `*mut ctx` conversion (green =
safe to convert that site; red = the cursor there must retire first). Do **not** run any
conversion off it this session.
**Accept:** `python3 rust/tools/q1c.py` runs and reports a site count; its docstring states
its role; the charter's family-6 precondition now points at a tool that exists.

### Step 3 — Convert the coefficient-only kernels (the proof)
**Goal:** the coefficient-only family is safe: kernels take `&mut [i16; N]`/`&[i16; N]`,
callers borrow their owned arrays, the shims and their `unsafe`/tags are deleted, and the
tree is byte-identical.
**Do, per kernel (strangler style, one closure per commit, S20):**
1. Change the kernel signature from `unsafe fn K(p: *mut i16, …)` to `fn K(x: &mut [i16; N], …)`
   (or `&[i16; N]` for read-only inputs like the `ff`/`mf` tables). The body already builds
   that slice via `from_raw_parts` — delete the round-trip, use the parameter directly.
2. Fix every caller: `K(local.as_mut_ptr())` → `K(&mut local)`; a call reading an `SMbCache`
   inline array borrows the field (`&mut cache.sCoeffLevel[range]` as `&mut [i16; N]` via
   `try_into`). If two coefficient blocks are borrowed at once from the same owner, split
   with `split_at_mut` (the decoder's coefficient paths do this — copy the pattern).
3. Delete the now-unused shim and its `PDctFunc`/`PQuant*`/`PScan*` typedef if nothing else
   references it (these are dispatch-table remnants; Phase 4 removed the analogous decoder
   ones). Remove the `// unsafe-cat: port-raw(Phase 9)` + `#[allow(unsafe_code)]` above it.
4. `gates.sh commit` — byte-identical, ratchet down.
**Facts / cautions:**
- The kernels' `# Safety` docs (alignment, length, disjointness) become compile-time
  guarantees of `&mut [i16; N]` — delete the doc clauses that the type now enforces, keep
  any that it does not (e.g. a value-range precondition).
- Some kernels are installed into the `SWelsFuncPtrList` dispatch table as
  `Some(WelsIHadamard4x4Dc)`. If a kernel is *only* ever called directly (not through the
  table), convert it and drop the slot. If it is still dispatched through a table this
  session does not retire, convert the direct calls and leave a thin `unsafe` shim for the
  table slot **tagged and noted** as belonging to family 5 (dispatch survivors) — do not
  expand the `unsafe` surface to keep a table alive.
- `svc_base_layer_md.rs` and `svc_encode_mb.rs` are the callers; `encode_mb_aux.rs` and
  `decode_mb_aux.rs` hold the kernels. Nothing in `src/common/` is in this family (its
  shims are plane/SAD, family 2).
**Accept:** every coefficient-only kernel takes array slices; `grep -c 'unsafe-cat: port-raw(Phase 9)'`
over `encode_mb_aux.rs`/`decode_mb_aux.rs` drops by the family's size; `gates.sh family`
byte-identical in both profiles; the ratchet is down by the deleted `unsafe fn`s/blocks.

### Step 4 — If short, drop from the end
Drop order: step 3's last few kernels (leave them tagged, note them for a later session),
then step 2 (q1c can be recovered by family 6's session — but recovering it now is cheap and
de-risks the phase, so keep it if at all possible). **Never drop step 1** — the map is the
session's main product.

### Step 5 — Close
`gates.sh family` (both profiles); the ratchet report; write the log entry (the census, the
detector, the family converted, the byte-parity result); update `phase9.md`'s session table
with what landed and the firmed-up family sizes. Report.

---

## Do not touch

| Item | Owner |
|---|---|
| `C-ABI` / `C-ABI(test)` tags (`src/api/`, the boundary) | frozen (Phase 8) |
| `SCREEN_CONTENT(dormant)` tags | Phase 10 |
| any `*mut sWelsEncCtx` parameter (the ctx conversion) | a later Phase 9 session; **blocked** until the cursors retire |
| plane (`*mut u8`), SMbCache/SMB, layer/pic/slice families | later Phase 9 sessions |
| the 22 dispatch survivors (unless a coeff kernel's table slot forces a note) | family 5 |
| perf, benches, spans, Miri (except the phase exit) | D-gate-1 / the phase close |

---

## Report back

1. **Commits** — hash + one line each.
2. **The census** — the family × file table, the 695/61 split reproduced, and any place the
   tree disagreed with this brief's numbers (quote both). This is the deliverable sessions
   B–J depend on.
3. **The detector** — that `q1c.py` runs and its current site count, and where it is
   documented.
4. **The coefficient family** — which kernels converted, which stayed (with the reason:
   plane-coupled, or table-dispatched), the exact tag-count drop, and the ratchet deltas.
5. **Byte parity** — `gates.sh family` result in both profiles; confirm zero moved bytes.
6. **Findings F101+**, and any fact in this brief that did not survive the tree.
7. **Firmed-up family sizes** for the charter's session table, so B can be scoped precisely.
