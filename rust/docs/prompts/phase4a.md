# Session prompt — Safety refactor, Phase 4a (dispatch de-virtualization + the recovery checkpoint)

You are starting **Phase 4a** of [`safety_refactor_plan.md`](../safety_refactor_plan.md).
Phase 2 closed on 2026-08-10; **D-seq-1** puts 4a here, before Phase 3.

**Read the plan's [§0 status preamble](../safety_refactor_plan.md) first** — it is one
screen and it is maintained. Then **§7.6 Standing working rules (S1–S19)**, which is
where the durable rules now live. This brief cites them by tag and does not repeat
them; where this brief and §7.6 disagree, §7.6 is the rule and this brief is wrong.

Expected span: **~2 sessions**. The phase has two halves that must not be traded
against each other — the de-virtualization, and the **checkpoint**. If you run short,
cut de-virtualization scope and keep the checkpoint whole: the checkpoint is what
five sessions of ledgered deficits have been waiting for, and it is what tells Phase 3
whether it has headroom.

---

## 1. State you inherit

| | |
|---|---|
| Tests | 404 debug / 402 release / 20 ignored |
| Sweeps | 341/341 both profiles (F3 retries per **S14** — the day-rate is load-dependent and was ≈1/47 on Phase 2's last day) |
| Ratchet | `unsafe_fn` 1346, `raw_ptr` 5167, `SHIM(` 154, `no_mangle` 24 — run `check`, never trust these numbers |
| Miri | whole library minus four skips, 267 tests, 81s; plus the differential files at phase exits |
| Ledger | decode ≈ **+17 / +10 / +10%**, encoder **+14.7% median** measured end-to-end — see §3 |
| Parked | `common/sad_common.rs` (14), `encoder/sample.rs` SATD (7) |

Phase 2's exit measurement is the thing to internalise: the **per-family ledger sum
and the direct Phase-2-start-to-end measurement agree** (≈+14% predicted, +14.73%
median measured on the encoder). The ledger is therefore a trustworthy instrument,
and the recovery you are about to measure can be read against it row by row.

## 2. The work — de-virtualization

Plan §5 Phase 4a has the full list. Order, and why:

1. **`mc.rs`'s consumers first**, under the **preserved D-perf-3 protocol** (it was
   written for exactly this task and survives D-perf-4's retirement of everything
   else): direct calls to `#[inline]` shims; **`Option`-unwrap semantics verified
   post-init** before any table is deleted; one interleaved pair on **both** benches
   per step. `mc.rs` is first because it is the largest single ledger row
   (decode +8.2/+7.2/+7.0%, encoder +7.6% median / +16.7% worst) and because if
   direct dispatch does not recover *it*, the whole recovery thesis is wrong and you
   want to know in the first session, not the last.
2. **The decoder kernel tables** — intra-pred arrays, `sBlockFunc`, `sDeblockingFunc`,
   `sMcFunc`, expand, and the cache-fill transmutes in `decode_slice.rs`. Phase 2
   deliberately left three untyped installer casts behind for you:
   `decoder_core.rs:1906` (the double-cast deblocking installer) and
   `decoder_core.rs:920-921` (two `mem::transmute` fn-pointer re-wraps).
3. **The encoder's ~55 CPU-dispatch members**, including `WelsInitSampleSadFunc`,
   which Phase 2 was explicitly forbidden to touch.

**Assert-map every table-init function before deleting it** (plan §5's own mitigation):
every replaced pointer provably pointed at exactly one function per config, and the
cheap way to keep that true is to assert it once at init and then delete the
indirection. `encoder_deblocking_table_installs_the_common_shims` in the differential
file is the shape.

### One thing to fix while you are in there, because it is yours

**F13's fourth site: `SWelsFuncPtrList` is self-referential.** `SetFastCodingFunc`
sets `sdf.pfMdCost = sdf.pfSampleSad.as_mut_ptr()` (and `SetNormalCodingFunc` the same
with `pfSampleSatd`), so `pfMdCost` points *into the same struct*. Every later
`&mut SWelsFuncPtrList` — and the encoder takes one constantly — reborrows the whole
struct and invalidates it, which Miri reports as UB on the next `pfMdCost` read. It is
in the Miri skip list as `--skip svc_mode_decision`. De-virtualizing the table is
precisely the fix: an enum or a direct call has no interior pointer to invalidate.
**Deleting that skip line is part of the work, not a follow-up.**

## 3. The checkpoint — the half that must not be compressed

### 3.1 Re-measure every ledger row

`perf_baseline.md` §Ledger has six open rows. Each one names the streams it moved and
by how much. Re-measure **each**, with dispatch direct, under **S1** (interleaved
pairs, 3+, medians) and **S2** (a fresh null first — the floor is per-session, it has
ranged ±1.8% to ±5.4% across sessions). Record recovery per row in the ledger's
`Phase 4 checkpoint` column, which has been sitting at *pending* since T4.

Two rows deserve naming:

- **`mc.rs`** is the thesis. Its deficit is fixed per-call shim overhead, and the claim
  has always been that direct dispatch lets LLVM inline the shim into the caller so the
  span arithmetic folds against caller constants and `from_raw_parts` disappears. If
  that is true, this row recovers most of the way. If it is not, **D-perf-3's fallback
  applies**: downgrade the remaining recovery claims to Phase 5/6 and say so in the
  ledger rather than leaving them as promises.
- **Spatial Ramps** is excluded from every verdict (**S2**) and is *still* your most
  sensitive instrument here. Session F measured it at **+226%** with readings that were
  consistent per binary across runs, and Phase 2's exit measured it at **+58/+60%**
  cumulative. Gradient content is the all-skip path — per-block scaffolding at its
  purest, with almost no real coding work to dilute it. **It cannot decide anything,
  but if direct dispatch is going to show up anywhere it will show up loudest here.**
  Measure it, record it, and keep it out of the verdict.

### 3.2 Re-attempt every parked family

Both parked families re-land here or get a second, dated verdict.

- **`common/sad_common.rs` (14 kernels, parked since `11f82d41`)** — proven,
  uninstalled, raw kernels still live. D-perf-2 spent Phase 2's one attempt on T8's
  exact-span trim (**S9**) and measured 1.39–2.97x; the box was closed for the phase.
  It opens here. **The lead worth trying, genuinely outside the rejected set:
  `PlaneCursor::new` is itself a large per-call cost at these block sizes** — paying
  the two constructions moved 4x4 from 1.61x to 4.93x — so hand these kernels
  **slices and offsets** instead of cursors. That is a convention change, which is why
  Phase 2 did not try it.
- **`encoder/sample.rs` SATD (7 kernels, parked session F)** — never swapped at all;
  the park was a projection from the SAD measurements rather than its own measurement.
  It deserves a real one.

**Rebuild the microbenchmark harness before trusting either** (**S3**, **S4**):
D-perf-2's harness read the in-tree kernel at 2.9–8.7x where T5's read 1.55–1.68x, and
moved raw `Sad8x4` 6.49 → 2.18 ns between two runs of the same binary.
`perf_baseline.md` §Parked records both the verdict and that caveat.

**Re-landing has a safety argument, not only a perf one.** Parked raw code is where
latent UB sits — F10 was found in it, three times — so a family that can re-land
should, and the bar is "no worse than the tripwire allows", not "free".

### 3.3 The exit deliverables

- Ledger rows updated with measured recovery, or explicitly downgraded per D-perf-3's
  fallback. No row stays `pending`.
- §Parked rows resolved: re-landed with numbers, or re-parked with a *new* dated
  verdict and its measurement.
- Plan §0 preamble refreshed (**S19**).
- **`prompts/phase3.md` written** — one brief ahead. Its read-first list is staged and
  waiting: **F4/F5/F7 for the decoder read side, F2 for the encoder write side**, plus
  **F13's `InitBits` site** (`kpBuf: *const u8` stored as `*mut` and written through —
  every honest caller is wrong, and `BsWriter` is where that signature dies).
- The 4a/4b fence held: **4b is after Phase 3**, and it is the config-dispatch enums
  (`pfWelsSpatialWriteMbSyn`, the RC-mode table) plus the strategy vtables — anything
  that touches what Phase 3 rewrites. Do not pull it forward.

## 4. Gates

Per **S14**, **S15**, **S16**, **S17** — unchanged from Phase 2's exit, with one
addition that is yours: this phase **deletes Miri skips**. `--skip svc_mode_decision`
should be gone by the end of it, and `--skip encoder_ext` may be if the dq-layer init
work lands here. Removing a skip is a gate *strengthening* and needs no ceremony
beyond the run being green.

The perf gate is D-perf-4's: one interleaved pair per bench per swap, full 3-pair
medians at the phase exit. But note the asymmetry — **this phase expects to make
things faster**, so a reading that shows *no* change is the surprising one and is
worth a disassembly look (**S1**) rather than a shrug.

## 5. Non-goals

No Phase 3 work (the bitstream layer is next-but-one, and 4b waits for it). No
structural rewrites — `SDqLayer`, `SMbCache` and the picture pool are Phases 5 and 6,
and touching them here will tangle the checkpoint's measurements beyond reading. No
fixing F8/F9/F11-class arithmetic (**S6** is parity, not repair). No new kernel
families — Phase 2's straggler sweep (**S18**) listed what is left and who owns it;
that list is in the session-G log entry and none of it is 4a's.
