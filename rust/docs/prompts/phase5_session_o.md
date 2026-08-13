# Phase 5, session O — T5.O: all of 5.5 under sprint gates

Governing: [`phase5.md`](phase5.md) §0/§5 verbatim; plan §7.4 — **D-gate-1 in
force** — and §7.6 (S30 is new; S20/S21/S24/S25/S28/S29 as always); the session-N
log entry §§5–7 and its hand-off. This file scopes the session and supersedes on
disagreement; fix disagreements in place. **Session scope enlarged per Eugene
(2026-08-12): 5.5 lands whole, both former parts.** Faces are ordered
riskiest-first; if wall time runs out, faces drop from the end at seam boundaries.
Counts were measured at `93e20e94` — **re-grep at each face's open, not once at
session start**: in a session this size, counts go stale mid-session (S24).

## 0. Start

1. Commit the inherited doc tail (S30, phase5.md §7's sprint form, the estimate
   re-plan, this brief).
2. Open per **S27**: session N ended accepted. Cheap subset if `rust/tools/` and
   the toolchain are unchanged. Last recorded: **470 / 464 / 20**, Miri **330**,
   census **59**, goldens **57**, `raw_ptr` **4436**. Recount.
3. **No perf measurement** (D-gate-1). Session N's +1.24% CB span, its S5 bisect,
   and the stashed binaries stand for the exit.

## 1. Face 0 — the pool `Id` niche, landed as code

`index: NonZeroU32` (`slot + 1`) in `safe/pool.rs`, per session N's hand-off —
a plain structural commit, no A/B (D-gate-1): it is the better representation
regardless, and the mechanism hypothesis rides on the ledger row for the exit to
adjudicate. Zero output moves or it reverts. If it is not small, list it and move
on.

## 2. Face 1 — the context decomposition closure, computed first

The phase's last large closure (§2.2.6): `SWelsDecoderContext` into subsystem
structs. **Compute and record the closure before any 5.5 edit** — it sizes every
commit in faces 2–3, and the decomposition decides which constructor lands where.
S25 enumeration alongside; the ~30-site `&mut *pCtx` class is gone (T5.G1), but
the subsystem borrows this face creates are new splitting decisions — R1's
field-precise-parameter rule is the tiebreaker.

## 3. Face 2 — allocation → constructors, the paramset store, F37

- Every remaining `WelsMallocz` allocation in `decoder_core.rs` becomes a real
  constructor with its `Drop` (T5.C3's template; the `MaybeUninit` shell at
  `decoder_context.rs:769` only where zeroed reach survives — prefer the
  constructor). **F19 per allocation: which line frees this?**
- **F37 fixes here**: `DestroyPicBuff` doesn't reset the reordering picture
  buffers where the C++ does — an uninit/re-init cycle leaves `sPictInfoList`
  naming slots of a freed pool. Restore the C++'s reset (parity, not invention);
  a cheap test on the uninit/re-init cycle is welcome.
- **The paramset store (P4)**: `pSps`/`pPps` self-referential fields →
  active-paramset *ids* + lookup at use; dequant caches addressed
  `[pps_idx][qp]`. S23: read the update paths before deriving anything live.
- S21 with force throughout; S29 spelling per function touched.

## 4. Face 3 — `Drop` teardown and the shell's retirement

The decomposed context gets its real constructor; `new_boxed()`'s `MaybeUninit`
shell is deleted (its comment names this step as its deleter); the remaining
decoder free-cascade entries die into `Drop` (R4). F19's question asked once more
over the whole file at the end: every allocation, one owner, one drop path.

## 5. Named, not started

- **The `pDec` step** (236 sites, 77 in `decode_slice.rs`) and everything behind
  it (`cur_and_ref`, 5.3b's punning + `SetRectBlock`) — next session's cluster
  with 5.6. Do not start from this session's tail.
- The NZC `*mut u8` cache family (167 uses) — 5.6's.

## 6. Gates — D-gate-1 sprint form

Per commit (~3 min): build both profiles + tests + ratchet + census. Once at
close: full battery — goldens 57 frozen, sweeps 341/341 both profiles, benches
bit-identical, Miri both probes; FAILs adjudicated per S14/S16; keep commits
small so a close-battery divergence bisects cheaply. **Do not edit the working
tree while the battery runs.**

## 7. Close

Log entry (the decomposition closure as computed, constructor inventory with
F19 answers, F37's fix, S21/S25 notes), phase5.md §5 marks and §0's rows,
hand-off: the `decode_slice` cluster session (`pDec` step → `cur_and_ref` +
5.3b → 5.6 whole), with its face order and any sizing this session's greps
produced.

## 8. Non-goals

No perf measurement (D-gate-1). No `pDec`/5.6/5.3b pulls. No F36 work. No golden
movement. No pool/threading (F12/P10). No `get_unchecked` (S8).
