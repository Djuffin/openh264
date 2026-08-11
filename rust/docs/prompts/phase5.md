# Phase 5 — decoder structural rewrite

Scope: plan §5's 5.1–5.6, in that order. Rules: plan §7.6 — S20/S21/S24/S25 bind
every struct edit here. Perf: §7.4 (D-perf-4, S2b). Before starting, read Phase 4b's
session B and C log entries (F19, F21) and this file whole. This file supersedes on
disagreement; fix disagreements in place. Counts below were measured at `263c71b2`;
re-grep before acting on any of them (S24).

Estimated 9–12 sessions. Per-session scope is the **S20 closure, not the file** —
compute it first, write it down, size commits by it. Enumerate the S25 re-entrancy
audit (who else reaches this object while I hold a borrow?) together with the
closure. In all constructor work, run F19's check per allocation: *which line frees
this?*

## 0. Session start

1. Commit any inherited doc tail first.
2. Control battery: `bash rust/tools/gates.sh full` **from the repo root**.
   `OVERALL:` is the verdict. Expected: **435 debug / 429 release / 20 ignored**,
   Miri **295**, sweeps 341/341 both profiles.
3. Recount every number you are about to rely on.

**F3** — known encoder MT race, not this phase's to fix; acquit and move on:
- Signature: `mt sm=3 n=600 t∈{2,4}`, wrong-length output (zero, short, long all
  count). Rate ≈ 1/800 configs under sustained load, ≈ 1/100–150 on susceptible
  configs.
- One hit → re-run that configuration 5×; a reproduction in isolation is valid
  evidence. Two different wrong lengths from one binary+config = race, not
  divergence.
- Two hits → alternate whole `mt` presets back to back, 12 per side, both binaries
  built once and swapped inside one loop, machine otherwise idle. Expect hits on
  both sides; the question is whether HEAD is worse.

## 1. Session A

### Face 1 — duplicate census (the T4b.3c/F21 class, enumerated up front)

Three instruments, none currently in `gates.sh`:

- `sh rust/tools/find_dup_types.sh` — currently a **252-line report**: 17 duplicate
  types, 34 aliases, **27 tables**, **37 value-divergent constants**. The
  within-decoder type duplicates sit on this phase's own targets:
  `SPartMbInfo` ×4 (`decode_slice.rs:467`, `mv_pred.rs:189`,
  `parse_mb_syn_cabac.rs:258`, `parse_mb_syn_cavlc.rs:304`), `SMbCache` ×2 in the
  decoder (`decoder_context.rs:546`, `decoder_core.rs:445`), `SPpsBsInfo` ×2,
  `SLastDecPicInfo` ×2, `SLayerInfo`, `SLogContext`, `SNalUnitHeader(Ext)`,
  `EWelsSliceType` ×3, `SDeblockingFunc` ×2.
- `python3 rust/tools/find_stub_bodies.py --dups` — duplicate fn bodies under one
  name.
- `grep -rn 'as \*mut _ as \*mut' src/` — **121** double casts (type laundering the
  `transmute` metric cannot see).

Do:
1. Run all three. Classify every hit:
   - **(a)** cross-codec namesake matching C++ (`SDqLayer` etc.) → allowlist. No
     renames (P14).
   - **(b)** within-codec duplicate declaration of one entity → unify. One commit
     per entity; divergences enumerated per copy (S21). Decoder-side entries owned
     by 5.2–5.5 unify now; encoder-side get listed with Phase 6 owners.
   - **(c)** value-divergent constant/table → record which value each consumer
     reads today and preserve per-consumer behaviour (S6). A naive merge changes
     output.
   - **(d)** legitimate same-name fns → allowlist.
2. Wire all three checks into `gates.sh`, zero-or-allowlisted (S22).
3. Zero bytes of output move in this face, or the commit that moved them reverts.

### Face 2 — P3 regression tests

One targeted test per decoder pointer-identity site, before any identity
conversion: boundary strength on a stream with duplicate-POC refs; EC self-copy;
the third site per plan §3 P3. Test additions only.

### Face 3 — 5.1 closure

Compute and record 5.1's S20 closure. First strangler commits only from a standing
start.

## 2. The steps (plan §5 order stands)

### 5.1 Picture & DPB
`Picture` re-struct (PaddedPlanes + `Vec`s), `PicPool`, `pic_queue.rs` recycling
predicate, `manage_dec_ref.rs`, `error_concealment.rs` identity sites.
- Decoder plane arrays are `[_; 4]` vs encoder `[_; 3]`
  (`decoder/picture.rs:102-105`, `encoder/picture.rs:70-71`). Check whether
  anything reads the fourth entry before fixing the count at three.
- **F21's fix (sub-32px chroma expand) lives in this step's files and is
  unpinned** — no corpus stream is narrower than 176px, so a regression is
  invisible to every gate. The pin is a narrow-frame decode asset: new golden
  rows, Eugene's call. If unauthorized, record the exposure in the step's log
  entry.

### 5.2 MbGrid
Kill the `sMb`/`SDqLayer` double path (P2); `SDqLayer` → `DqLayerState` with owned
`MbGrid`; re-point `parse_mb_syn_*` cache fills (~40 signatures; the 30-entry
scratch caches become `&mut` locals passed down).
- Expect the phase's largest closure; `SDqLayer` is embedded and asserted widely.
- `SDqLayer::pBitStringAux` (`*mut BsReader`) retires here; `cabac_decoder.rs`'s
  `SHIM(phase5)` accessor dies with it.
- The decoder's `SHIM(phase2)` kernel adapters retire as 5.2–5.4 convert their
  callers (154 markers crate-wide; the decoder share goes here).

### 5.3 Neighbor & MV
`mv_pred.rs`: punning → byte ops; `SetRectBlock` → typed generic on the grid;
colocated reads via `cur_and_ref`.

### 5.4 Deblocking driver
`decoder/deblocking.rs`: `SDeblockingFilter` holds `PicId`s + per-MB plane
cursors; identity compare per P3.

### 5.5 decoder_core.rs
Allocation → constructors (`AllocPicture` → `Picture::new`), paramset store (P4),
context decomposition (§2.2.6), `Drop` teardown, `Default` derives.
- S21 with force: the decoder context embeds its buffers **by value**, so every
  owned field lands inside `mem::zeroed` reach. The `MaybeUninit` shell +
  `new_boxed()` exists at `decoder_context.rs:769`; extend it per owned field;
  replace it with a real constructor at the end of this step.
- F19's check runs here, per allocation.
- `SWelsDecoderContext` has no `assert_size!` and no offset pins; don't look for
  an instrument that isn't there.

### 5.6 decode_slice.rs (last, per P1)
Including the EC MC paths. Delete the remaining decoder shims. Decoder modules get
`#![deny(unsafe_code)]` one by one. No `SBitStringAux` shell exists (deleted at
T3.4).

## 3. Gates and exit

Per face: full battery; decoder goldens frozen; sweeps 341/341 both profiles;
3-pair interleaved medians per seam (S2b: a median outside the null band gets more
pairs before it gets a mechanism); Miri; ratchet regenerated per S16 with deltas
named.

Exit: frame-count parity and the `#[ignore]` set unchanged; T3.0's 2316-row golden
table green in both profiles (T7 stays deferred); decoder `src/` unsafe-free;
**every §7.4 ledger entry whose shims died in this phase must clear**. This phase
collects 4a's downgraded decode rows (≈ +17.8/+10.1/+9.6% cumulative; ~7 points of
CB headroom under the tripwire). The mechanism is constant dimensions reaching the
kernels, so flat mid-phase bench readings are expected — the ledger is the
instrument that moves. S19 at exit: refresh §0, write `prompts/phase6.md`, stamp
this brief historical.

## 4. Metrics inherited

- `transmute` reads **4: all prose, zero calls**. Don't chase it.
- `assert_size!(SWelsFuncPtrList)` cannot see dispatch leave (`Option<Box<_>>` is
  pointer-sized); the **ratchet** is the instrument. Phase 4b ledger:
  `raw_ptr` 5001 → 4815, `unsafe_fn` 1286 → 1250.
- `SHIM(` 157: `phase3` = 2 (Phase 6's), `phase5` = 1 (dies in 5.2), rest
  `phase2`.

## 5. Non-goals

No Phase 6 pulls: encoder `SMbCache`, `SSlice` layout, the free cascade,
`wels_encoder_ext.rs` internals. No parked-family reopening (6.3's). No
F8/F9/F11-class fixes (S6). No `get_unchecked` (S8). No golden movement except the
F21 asset if authorized. No pool/threading edits (F12/P10). No F3 work beyond §0's
protocol.

Cheap and welcome if passing: delete `pfSetNZCZero`
(`encoder/wels_func_ptr_def.rs:385`) — one slot, one unconditional constant;
takes `assert_size!(SWelsFuncPtrList)` to 1152 and removes the last reason
`encoder/deblocking.rs`'s duplicate `WelsNonZeroCount_c` exists. Encoder-side
(6.5's by rights), listed because it is ~6 lines.
