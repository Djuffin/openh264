# Phase 9 — the safety endgame (charter)

*Written 2026-08-22 by the steward at `acebd103` (Phase 8b closed). This is the map the
phase executes against; individual sessions get their own briefs. Every number here was
grepped against the tree at that commit — re-grep before quoting (S24).*

## 1. What Phase 9 is

Phases 5 and 6 gave the decoder and encoder **owned** data — `Vec`, `Option<Box>`,
arrays, handles — in place of raw allocations. Phase 5 then made the decoder's **access**
to that data safe (borrows, slices, `PicId`/`MbGrid` handles) and the decoder now carries
`#![deny(unsafe_code)]` on all 22 modules with essentially zero `unsafe-cat` tags. Phase 9
does for the **encoder** what Phase 5 did for the decoder: retire the raw cursors the
encoder still hands around, so its `unsafe` disappears rather than being tagged.

The encoder is **already** under `#![deny(unsafe_code)]` on all 39 files. It compiles only
because every remaining unsafe site carries a tagged `#[allow(unsafe_code)]`. The tags are
the work queue. At `acebd103`:

| tag | count | what it is | owner |
|---|---|---|---|
| `port-raw(Phase 9)` | **695** | raw-pointer signatures and unsafe bodies below the boundary | **this phase** |
| `cursor` | **61** | owned-storage cursor accessors (S28/S40 tests) + the 22 dispatch survivors | **this phase** |
| `C-ABI` | 45 | the frozen FFI boundary | stays (Phase 8) |
| `MT` | 21 | the fork/join seam's neighbourhood | **this phase** (retires with the ctx split) |
| `SCREEN_CONTENT(dormant)` | 8 | the FME search half | Phase 10 |
| `C-ABI(test)` | 5 | the Miri driver and P13's probes | stays |
| `send-seam(Phase 9)` | 1 | D-mt-1's one `unsafe impl Send` | **this phase** |

Definition of done (plan section 2.3, unchanged): every module outside `src/api/` is
unsafe-free and raw-pointer-free; `*mut`/`*const` survive only in the `#[repr(C)]` ABI
types and the boundary thunks; `libc` is gone; the crate carries `#![deny(unsafe_code)]` at
the root with the only `#[allow]` on `src/api/`.

## 2. The shape of the 695 (measured, not assumed)

Classified by the signature each tag sits above:

| family | count | what it is | retires by |
|---|---|---|---|
| **body-only `unsafe fn`** | **419** | `unsafe fn` (often `&mut self`) whose *signature* is safe; unsafe because the body derefs a raw cursor or calls an unsafe fn | falls out as its callees go safe — **not converted directly** |
| **`*mut sWelsEncCtx` param** | **120** | the F66 conversion — a `*mut sWelsEncCtx` parameter becoming a borrow | after the cursors (F66) |
| **`*mut u8` plane/byte** | **51** | plane cursors and prediction buffers | the plane conversion |
| **layer/pic/slice ptr** | **33** | `*mut SDqLayer`/`SPicture`/`SSlice` | the picture/layer conversion (F73) |
| **other raw sig** | **31** | fn-pointer installers, misc | case by case |
| **coeff cursor** | **25** | `*mut i16`/`*const i16` DCT/quant/scan buffers | the coefficient conversion |
| **SMbCache/SMB** | **15** | `*mut SMbCache`/`*mut SMB` per-macroblock metadata | the MB-metadata conversion |

The decisive fact (F66, phase6_findings): converting a `*mut sWelsEncCtx` parameter to
`&mut sWelsEncCtx` is **blocked until the raw cursors are gone**. A `&mut ctx`
function-entry retag invalidates the *whole* 65-field context under Stacked Borrows, so any
caller holding a context-derived cursor across the call is UB (93 such sites in 28 callers;
session J made the conversion, Miri refused it, it was reverted). The port's whole idiom is
"derive a cursor from ctx, call something, use the cursor" — sound only while the cursor is
**raw** (a raw retag pops only the offsets it touches). So the order is forced:

> **cursors first (planes, coeff, MbCache, layer/pic) -> then the `*mut ctx` conversion ->
> then the 419 body-only fns go safe as their callees do.**

The kernels the cursors feed are **already safe**. `mc.rs`'s `copy_4x4`,
`encode_mb_aux.rs`'s `dct_4x4`/`quant_4x4`, `sad_common.rs`'s SADs all take
`&PlaneCursor`/`&mut [i16; N]` today; the `unsafe` is the **shim** that wraps them for a
raw-pointer caller (`copy_shim(pDst: *mut u8, ...)` -> `from_raw_parts`). A family converts
by making its **callers** hold the safe type; the shim then has no raw caller and is
deleted, and the shared `common/` kernel loses its last raw entry point. This is why
`common/`'s 9 un-denied files (mc, sad_common, deblocking_common, intra_pred_common,
copy_mb, expand_pic, wels_common_defs, wels_trace, cpu_core) are **not an independent
task** — they go `deny` as the encoder callers that reach their shims convert.

## 3. The safe vocabulary already exists

Do not invent types. Phase 5 built and proved these; the encoder reuses them:
- `safe::plane::{PaddedPlane, PlaneCursor, PlaneCursorMut}` — plane roots and bounded
  cursors (S37/S40 retag-stable roots).
- `safe::pool::{Pool, Id}` — owned slots addressed by handle (grow/shrink added in 8b.C).
- `safe::mb_grid::MbGrid` — single-owner per-MB metadata (`forbid(unsafe_code)`).
- The encoder's picture pool (`encoder::picture`) and layer storage already own their data
  (Phase 6); what is raw is the *access*.

## 4. The families, in dependency order (the session plan)

Each family is a Phase-5-style rollout: convert the callers to hold the safe type, delete
the shims, retire the tags, Miri-verify. Sizes are estimates; the census (session A) firms
them.

1. **Coefficient cursors** (25 tags + the `PDctFunc`/`PQuant*`/`PScan*` typedefs) — a closed
   residual pipeline (DCT -> Hadamard -> quant -> scan -> NZC) whose kernels are already
   safe. **Session A's proof family** — bounded, self-contained, no context borrow.
2. **Plane cursors** (51 `*mut u8` + F73's picture-accessor family, 32+68 sites) and with
   them `common/`'s mc/sad/deblocking_common/intra_pred_common shims. The largest; the
   reconstruction and source planes reached through the pool. ~2–3 sessions.
3. **SMbCache / SMB** (15 + Phase 6's 72 kernels-take-slices note) — per-macroblock
   metadata; the encoder's `MbGrid` analogue. ~1–2 sessions.
4. **Layer / picture / slice pointers** (33 + the 61 `cursor` accessors minus the 22
   survivors) — `SDqLayer`, `SPicture`, `SSlice` reached as owned storage. ~1–2 sessions.
5. **The 22 dispatch survivors** — de-virtualize the five self-referential `SWelsFuncPtrList`
   slot types (`PDeblockingBSCalc`, `PDeblockingFilterSlice`, `PMotionSearchFunc`,
   `PSearchMethodFunc`, `PLineFullSearchFunc`), the way Phase 4a de-virtualized `pfMdCost`.
   Independent of the cursor spine. ~1 session.
6. **The `*mut sWelsEncCtx` conversion** (120) — unblocked once 1–4 land. Run session J's
   `q1c.py` aliasing-hazard detector as the **precondition** (F66's instruction), and split
   the context into sub-borrows (section 2.2.6) only where a caller must hold a sub-borrow
   across a call. The MT `send-seam` and the 21 MT tags retire here (D-mt-1). ~2–3 sessions.
7. **Crate-root cleanup** — `#![deny(unsafe_code)]` at the root (api keeps its tagged
   allows); drop `libc`; clippy `pedantic` triage; the module `deny`s become redundant with
   the root and the ratchet's job narrows to watching `api/`; final unscoped Miri and (if
   built) fuzz marathons. ~1 session.
8. **Inherited findings** — fold in where they fit, or a dedicated session: **F91** (upstream
   writes into the caller's `const` bitstream in parse-only SVC — the port does not; decide
   whether to match), **F92** (subset-SPS rewrite bounds — reachability probe), **F93** (four
   parse-only rows on damaged streams), **F100** (`LogStatistics`/`TraceParamInfo` empty
   bodies — pure `WelsLog`), **F84** (two orphaned threaded-decoder functions + 18 pad
   kernels — S18 deletions), **F86's open half** (the unchecked `pShortRefList[iRefIdx + 1]`
   write). Referees for these are built and green (8b).
9. **The end-of-phase decisions** — **D4** (workspace split into a `#![forbid]` core + the
   cdylib boundary — now a mechanical extraction, Phase 8 made the boundary precise) and
   **D5** (the Wels/Hungarian -> idiomatic rename) go to the user with the phase's final
   state in hand.

## 5. Estimate

**10–14 sessions.** This is the encoder's equivalent of Phase 5 (the decoder took 29 from
scratch), but the encoder starts with ownership done (Phase 6), the vocabulary built (Phases
1/5), and the hazards already found and named (F66/F73) — so it is closer to Phase 6's
10-session scale than Phase 5's 29. The uncertain part is family 6 (the ctx conversion under
F66); families 1–5 are mechanical caller-side conversions against proven types.

## 6. Standing rules that bind every session

- **D-gate-1 / D-gate-2**: no mid-phase perf measurement; one battery at the phase close;
  Miri once at the phase exit, unscoped (both codecs' modules move here).
- **D-gate-3 does not apply** — that was Phase 8b's parity-gating. Phase 9 changes code that
  byte gates and Miri both watch, so it runs `gates.sh family` (sweeps) per session close and
  `gates.sh commit` per commit; Miri at the phase exit. *(A session that only deletes shims
  and converts callers byte-for-byte may justify commit-level gating and say so — but the
  sweep is cheap insurance against a cursor conversion that moves a byte.)*
- **The referee is the tree, not the reference**: these are safety conversions with no
  intended behaviour change, so **every commit is byte-identical** on the goldens, the
  corpus, and both benches. A moved byte is a defect, bisected across the session's commits.
- **S20** (signature-reachability closure as the commit unit), **S37/S40** (a converted
  cursor's root must be retag-stable, with the Miri test), **D-exit-1** (any *new* raw
  signature is tagged; the count only goes down), **the ratchet decreases or is rebaselined
  with a reason in the same commit**.
- **Run `q1c.py` (the F66 detector) before any `*mut ctx` conversion**, not after.

## 7. Exit conditions

1. Every module outside `src/api/` carries `#![deny(unsafe_code)]` **and has zero
   `#[allow(unsafe_code)]` items** except the lawful boundary categories, which by Phase 9's
   end are only `C-ABI`/`C-ABI(test)` (all `port-raw`, `cursor`, `MT`, `send-seam` retired)
   and `SCREEN_CONTENT(dormant)` (Phase 10's).
2. `#![deny(unsafe_code)]` at the crate root; `libc` removed from `[dependencies]`; `*mut`/
   `*const` appear only in the ABI types and the boundary thunks (a grep census proves it).
3. `gates.sh exit` PASS unscoped — sweeps both profiles, both benches bit-identical, Miri
   `--lib` whole library, the differential tests, the gtest allowlist still exactly the 7
   Phase 10 rows + D-poc-1.
4. The perf position restated against D-perf-4's +25% tripwire, and **D-perf-6's parked
   recovery** taken (the Phase 5/6 parked families revisited now that dispatch is direct) or
   explicitly re-deferred with the user.
5. D4 and D5 put to the user with the final state; the ratchet's remaining job (watching
   `api/`'s size) documented.

## 8. Sessions

| session | scope | brief |
|---|---|---|
| **A** | scope + census + the `q1c.py` detector wired; the coefficient-cursor family as the proof | [`phase9_session_a.md`](phase9_session_a.md) |
| B–C | the plane-cursor family + `common/`'s plane shims | — |
| D | SMbCache/SMB | — |
| E | layer/picture/slice + the cursor accessors | — |
| F | the 22 dispatch survivors | — |
| G–H | the `*mut ctx` conversion (q1c precondition) + the MT seam/Send flip | — |
| I | inherited findings (F91/F92/F93/F100/F84/F86-open) | — |
| J | crate-root cleanup, libc, clippy, the Miri/fuzz marathon; D4/D5 to the user | — |
