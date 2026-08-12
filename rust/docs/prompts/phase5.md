# Phase 5 — decoder structural rewrite

Scope: plan §5's steps 5.1–5.6, in order. **Sessions A (duplicate census, P3 identity
tests, 5.1 closure) and B (5.1's F21 pin and S25 audit) are done — work starts at
5.1's plane conversion, §1 step 3.** Rules: plan §7.6 — S20/S21/S24/S25
bind every struct edit here. Perf: §7.4 (D-perf-4, S2b). Before starting, read the
Phase 5 session-A log entry (the 5.1 closure is its §5) and
[`phase5_findings.md`](../phase5_findings.md) F22. This file supersedes on
disagreement; fix disagreements in place. Counts below measured at `f974e0e8`;
re-grep before acting on any of them (S24). Estimated 7–10 sessions remain.

Per-session scope is the **S20 closure, not the file** — compute it first, write it
down, size commits by it. Enumerate the S25 re-entrancy audit (who else reaches this
object while I hold a borrow?) together with the closure. In all constructor work,
run F19's check per allocation: *which line frees this?*

## 0. Session start

1. Commit any inherited doc tail first.
2. Control battery: `bash rust/tools/gates.sh full` **from the repo root**.
   `OVERALL:` is the verdict. Expected: **443 debug / 437 release / 20 ignored**,
   Miri **304**, sweeps 341/341 both profiles.
3. Recount every number you are about to rely on.

The census gate runs at commit level (`rust/tools/census.sh` against
`rust/tools/census_allowlist.txt`): a new duplicate declaration or inferred-target
double cast fails the build. When a step unifies an allowlisted entry, remove its
line; new allowlist entries carry their class (a: cross-codec namesake — no renames,
P14; b: within-codec duplicate — unify, divergences per copy, S21; c: value-divergent
constant/table — preserve per-consumer behaviour, S6; d: legitimate same-name fn) and
owning step.

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

## 1. Step 5.1 — Picture & DPB (steps 1–2 done at session B; the conversion is next)

`SPicture`'s planes become owned (`PaddedPlanes` + `Vec`s); `PicPool`;
`pic_queue.rs` recycling predicate; `manage_dec_ref.rs`; `error_concealment.rs`
identity sites. Five P3 identity tests exist (`deblocking.rs` ×3,
`error_concealment.rs` ×2, all same-POC) — read them before touching either file.

Order inside the step:

1. ~~F21 pin first.~~ **Done, T5.B1 (`0fd0c9cc`).** Three assets, goldens 53 → 56,
   regenerable by `rust/tools/make_narrow_assets.py --check`. **Only
   `narrow_16x16_idr_lost` covers F21** — the two clean narrow rows are green under
   a revert of the fix, because the divergent copy's one call site is
   `WelsInitRefList`'s concealment prefetch, which no cleanly-decoding stream
   reaches. Coverage proven by that revert, not asserted.
2. ~~The S25 hazard.~~ **Done, T5.B2 (`ff12e966`) — and this brief was wrong about
   what it was.** `SPicture::unref` had one caller, its own unit test, and no C++
   counterpart: deleted, not restructured. The audit found **nine live functions**
   in `manage_dec_ref.rs` holding `&mut *pCtx` / `&mut *pRefPic` across re-entrant
   calls (`pRefPic` is `&mut ctx.sRefPic`, a *subfield* of the context, so the two
   overlap); all now name `(*pCtx)` / `(*pRefPic)` per use, with the rule written at
   `SetUnRef`. **F13's `manage_dec_ref` Miri skip is gone** — six
   `as_ptr()`/`as_mut_ptr()` list shifts behind it, and three test defects.
3. **The conversion — session C's whole job.** Per the closure in the session-A log
   §5, corrected by session B's recount: nothing embeds the decoder's `SPicture` by
   value (every holder is a pointer; the ten-file table re-greped and unchanged); it
   has no `assert_size!` and no offset pins; **`iPlanes` is written-never-read in
   the port *and* in the C++ decoder**, so fixing the count at three is subtraction;
   F19's check is clean on all eight allocations today — keep it true as
   `AllocPicture`/`FreePicture` become `Picture::new`/`Drop`. `PaddedPlane::new` /
   `from_parts` (`safe/plane.rs`) are the constructors, and `new`'s doc already
   names the `128` fill `AllocPicture` does. Miri watches `manage_dec_ref` now.
4. **Not audited yet, and each is its own S25 enumeration**: `pic_queue.rs`
   (`SPicBuff.ppPic` is `*mut *mut SPicture` and the recycling predicate walks it
   while `pCtx->pDec` points into the same array), `deblocking.rs`,
   `error_concealment.rs`. Do them with their conversion, not before it.

## 2. Step 5.2 — MbGrid

Kill the `sMb`/`SDqLayer` double path (P2); `SDqLayer` → `DqLayerState` with owned
`MbGrid`; re-point `parse_mb_syn_*` cache fills (~40 signatures; the 30-entry
scratch caches become `&mut` locals passed down).
- Expect the phase's largest closure; `SDqLayer` is embedded and asserted widely.
- `SDqLayer::pBitStringAux` (`*mut BsReader`) retires here; `cabac_decoder.rs`'s
  `SHIM(phase5)` accessor dies with it.
- The decoder's `SHIM(phase2)` kernel adapters retire as 5.2–5.4 convert their
  callers (154 markers crate-wide; the decoder share goes here).
- **Answer F22's reachability question here**, while the cache-fill closure is
  open: can `pDec` be null on the CABAC parse path? The answer decides whether
  5.3's guard divergence (28 null guards in `mv_pred.rs`, 0 in the CABAC copies)
  is a latent crash or dead code. Record it in the log either way.

## 3. Step 5.3 — Neighbor & MV

`mv_pred.rs`: punning → byte ops; `SetRectBlock` → typed generic on the grid;
colocated reads via `cur_and_ref`.
- **F22 unifies here**: `parse_mb_syn_cabac.rs` re-translated six `mv_pred`
  functions and dropped every `pDec` null guard (28 vs 0). Unify onto one copy
  with divergences enumerated per copy (S21); which guard semantics survive
  depends on 5.2's reachability answer.

## 4. Step 5.4 — Deblocking driver

`decoder/deblocking.rs`: `SDeblockingFilter` holds `PicId`s + per-MB plane
cursors; identity compare per P3 (the three deblocking identity tests gate this).

## 5. Step 5.5 — decoder_core.rs

Allocation → constructors, paramset store (P4), context decomposition (§2.2.6),
`Drop` teardown, `Default` derives.
- S21 with force: the decoder context embeds its buffers **by value**, so every
  owned field lands inside `mem::zeroed` reach. The `MaybeUninit` shell +
  `new_boxed()` exists at `decoder_context.rs:769`; extend it per owned field;
  replace it with a real constructor at the end of this step.
- F19's check runs here, per allocation.
- `SWelsDecoderContext` has no `assert_size!` and no offset pins; don't look for
  an instrument that isn't there.

## 6. Step 5.6 — decode_slice.rs (last, per P1)

Including the EC MC paths. Delete the remaining decoder shims. Decoder modules get
`#![deny(unsafe_code)]` one by one. No `SBitStringAux` shell exists (deleted at
T3.4).

## 7. Gates and exit

Per face: full battery; decoder goldens frozen (the F21 rows included, once
landed); sweeps 341/341 both profiles; 3-pair interleaved medians per seam (S2b: a
median outside the null band gets more pairs before it gets a mechanism); Miri;
ratchet regenerated per S16 with deltas named.

Exit: frame-count parity and the `#[ignore]` set unchanged; T3.0's 2316-row golden
table green in both profiles (T7 stays deferred); decoder `src/` unsafe-free;
**every §7.4 ledger entry whose shims died in this phase must clear**. This phase
collects 4a's downgraded decode rows (≈ +17.8/+10.1/+9.6% cumulative; ~7 points of
CB headroom under the tripwire). The mechanism is constant dimensions reaching the
kernels, so flat mid-phase bench readings are expected — the ledger is the
instrument that moves. S19 at exit: refresh §0, write `prompts/phase6.md`, stamp
this brief historical.

## 8. Metrics inherited

*(Re-greped at session B's exit. S24 still binds — recount before acting.)*

- `transmute` reads **4: all prose, zero calls**. Don't chase it.
- The **ratchet** is the instrument, not struct sizes. Phase 5 so far:
  `raw_ptr` 4815 → **4597** (session A, by deletion; session B flat),
  `unsafe_block` 613 → 618 → **616**, `unsafe_fn` 1250 → **1249**.
- Gates: **443 / 437 / 20**, Miri **304**, sweeps 341/341 both profiles, decode
  goldens **56 rows**.
- **Miri skips are 2, not 3**: `wels_thread_pool` (F12, Phase 7) and `encoder_ext`
  (F13, Phase 6). `manage_dec_ref` came off at T5.B2.
- Census gate state: duplicate types 10, aliases 31, tables 17, inferred-target
  double casts 0, duplicate-body groups 198 (ratcheted), 61 allowlisted entries.
  Remaining within-decoder entries are allowlisted with their owning steps.
- `SHIM(` 157: `phase3` = 2 (Phase 6's), `phase5` = 1 (dies in 5.2), rest
  `phase2`.

## 9. Non-goals

No Phase 6 pulls: encoder `SMbCache`, `SSlice` layout, the free cascade,
`wels_encoder_ext.rs` internals. No parked-family reopening (6.3's). No
F8/F9/F11-class fixes (S6). No `get_unchecked` (S8). No golden movement beyond the
authorized F21 rows. No pool/threading edits (F12/P10). No F3 work beyond §0's
protocol.

Cheap and welcome if passing: delete `pfSetNZCZero`
(`encoder/wels_func_ptr_def.rs:385`) — one slot, one unconditional constant;
takes `assert_size!(SWelsFuncPtrList)` to 1152 and removes the last reason
`encoder/deblocking.rs`'s duplicate `WelsNonZeroCount_c` exists. Encoder-side
(6.5's by rights), listed because it is ~6 lines.
