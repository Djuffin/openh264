# Session prompt — Safety refactor, finishing Phase 2 (T7 part 1 → T7 part 2 → T9)

> **SUPERSEDED — HISTORICAL.** Phase 2 closed 2026-08-10. The durable rules this
> brief carried now live in [`../safety_refactor_plan.md`](../safety_refactor_plan.md)
> §7.6 (S1–S19); the phase's state is in that document's §0 preamble; the next phase's
> brief is [`phase4a.md`](phase4a.md). Kept for the history in its state tables and
> for the derivations behind the hoisted rules — **do not execute it.**

You are finishing **Phase 2** of `rust/docs/safety_refactor_plan.md`: the last two
kernel families and the phase exit. Expected span: **two sessions** (F: T7 part 1;
G: T7 part 2 + T9), with T9 allowed to spill into a short third if part 2 runs long —
never compress T9 to make it fit.

Document chain, and where authority lies: `prompts/phase2.md` governs conventions
(two commits per family, shim/naming rules §3.2, non-goals §6);
`prompts/phase2_continue.md` **§2 holds the live rule set R-a…R-p** (still the
operative home until T9 hoists the durable ones into plan §7.6) and its state table
holds the history; **this file owns the remaining tasks and supersedes both on any
disagreement.** The regime is **D-perf-4** (plan §7.4 v3: swap-and-ledger by default,
+25%-median-cumulative tripwire parks a family, one interleaved pair per bench at
commit B, no optimization boxes, byte-exactness untouchable) and the schedule beyond
this phase is **D-seq-1** (Phase 4a next, before Phase 3).

State at **`12440d34`** (session F complete — tree clean, all gates green):

| | |
|---|---|
| Tests | 399 debug / 397 release / 20 ignored; differential file 16/16 under Miri |
| Sweeps | 341/341 both profiles after F3 retries — **F3's signature widened again, see §4** |
| Ratchet | `unsafe_fn` 1361, `raw_ptr` 5200, `SHIM(` 127, `no_mangle` 24 — run `check`, never trust remembered numbers |
| Cumulative ledger | decode ≈ **+17 / +10 / +10%**, encoder ≈ **+13% median** (F-1 added +2.1) — G-1's tripwire arithmetic starts from these |
| Parked | T5-sad (14) **+ the seven SATD kernels** (session F), both re-attempting at Phase 4a |

**Counts:** `encoder/get_intra_predictor.rs` measured **27** `pub unsafe fn` when this
brief was written — but session F recounted *both* its families up (21+installer, 9
not 6, with five raw bodies found living in `svc_encode_mb.rs`). Recount at session
start, and grep for stray bodies in *adjacent* files, not just the named one.

---

## 1. Read first

1. This file, then `prompts/phase2_continue.md` §2 (rules R-a…R-p — R-e, R-l, R-m,
   R-n and the golden-run/F10 additions are the ones this stretch leans on).
2. `rust/docs/safety_refactor_log.md` — sessions E and **F** entries in full (the
   probe lesson and per-session noise floor; F's recounts, its F10-second-instance
   correction, and the F3 sixth-measurement protocol run — the widened signature you
   gate under). Session D for the instrument rules if you need their derivations.
3. `rust/docs/phase2_findings.md` — **F8** and **F11** (the arithmetic-parity
   precedents, decoder- and encoder-side), **F10 both instances** (raw-side
   differential buffers are **`(h+1)*stride`** — the finding as written, not "whole
   rows" as remembered; session F paid for the difference), and **F3's current
   signature** (§4 below).
4. `perf_baseline.md` — §Ledger and §Parked (you will add rows to both), §Phase 2 T8
   (the exact-span trim and the two microbench lies), the T4 row-walker table
   (negative results you must not re-propose).
5. Plan §5 Phase 4a and §7.4 D-perf-4/D-seq-1 — so T9's hand-off artifacts say the
   right things.

## 2. Session F — DONE (`12440d34`, eight commits, no deviations)

Outcomes, compressed (the log's session F entry has the full record):
**F-1** `encode_mb_aux.rs` — 21 kernels + installer behind 21 shims, fixed-array
signatures, the two strided Hadamard reads carrying exact-reach types (`[i16; 49]`,
`[i16; 241]`); encoder +2.1% median / +9.9% worst, ledgered; cumulative encoder
now ≈ +13%. **F-2** — the recon IDCT family was **9** kernels, not 6 (five raw bodies
lived in `svc_encode_mb.rs`); noise-level both benches; **F11** recorded
(`WelsIHadamard4x4Dc` i16 adds panic in debug above ±2047 where the C++ wraps —
F8's class, reproduced not repaired). **F-3** — seven SATD kernels proven and
**parked**, no commit B, §Parked row names Phase 4a. **F10 second instance**: the
composites bump from sub-block anchors, reaching `(w-4) + h*stride` past the block —
"whole rows" as remembered was not enough; raw-side differential buffers are
**`(h+1)*stride`**, per the finding as written. The phase-exit Miri protocol was run
mid-session (that's how the F10 instance surfaced) — it still re-runs at T9.
**Spatial Ramps read +226% with per-binary-consistent readings** — excluded from all
verdicts per the standing rule, recorded in the baseline as Phase 4a checkpoint data
(gradient content is the per-block-scaffolding path at its purest).

The original session-F instructions are spent and removed; the durable rules they
carried live in `phase2_continue.md` §2 and the F10/F11 findings.

## 3. Session G — T7 part 2: `encoder/get_intra_predictor.rs` (27), then T9

### G-1. The last family

- **27 kernels** (recounted; inventory again): I16x16 luma (V/H/DC/planar + DC
  variants), I4x4 luma (the directional set + DC variants), chroma — the encoder
  MD's candidate generators. **Two surfaces per kernel, and they differ:** the
  *reference* side reads the recon plane at `-1` column / `-stride` row
  (availability-gated, PADDING-legal — T3's span-helper shape, reuse its formula
  approach); the *destination* is a **packed** prediction buffer (`[u8; 256]` /
  `[u8; 16]`-per-block — T5-intra's lesson: packed dst is what distinguishes these
  from the strided decoder cousins converted in T3; per-kernel reference shapes, not
  a shared one, so no contract claims reach it doesn't have).
- **Name collision discipline (third occurrence):** decoder T3 names and
  common-intra T5 names overlap this file's — never unify, never cross-delete.
- **R-d exhaustively:** MD calls these under availability masks; sweep every
  flag combination — T3 found kernels that ignore flags they take.
- **Commit B pair + tripwire:** call density here is per-mode-per-MB (an order less
  than SAD's per-candidate), so T3's wash is the base expectation — but this is the
  encoder, so let the pair decide, **starting the arithmetic from encoder ≈ +13%
  median cumulative** (post-F-1): past +25% median on any stream → park with the
  SATDs (Phase 4a re-attempt); under → swap and ledger. Either way this is the
  phase's last swap decision — record it with its arithmetic shown.

### G-2. T9 — the phase exit, in full

Run in this order (document work last, so the code state it describes is final):

1. **Straggler sweep:** grep for any arch-suffixed (`_sse2|_neon|_mmi|_lsx|_ssse3`)
   or table-installed kernel the per-family passes missed; every hit is either
   converted, deleted-dead, or explicitly listed in the log with its owner phase.
2. **Counts finalized:** `SHIM(phase2)` total and per-file; `no_mangle` (kernel
   files must contribute **zero**; what remains should be `api/` exports plus
   whatever `wels_encoder_ext.rs` carries — list them); full ratchet regenerate.
3. **Miri, widened:** the standing `--lib safe::` + both differential files with
   `scale()` — **and implement the session-B item**: extend `gates.sh`'s Miri step
   to the port's own unit tests (the mc alias test exercised UB invisibly to the
   `safe::`-only gate; a gate change is legitimate exactly here, at the phase
   boundary). If port tests surface new Miri hits, they are findings
   (record, accommodate per F10 precedent, don't fix bodies).
4. **Final perf:** full 3-pair interleaved medians, both benches (this is the one
   place the heavyweight protocol still runs); per-family delta table; append the
   Phase 2 column to `perf_baseline.md`; reconcile §Ledger (rows stay open, Phase 4a
   checkpoint pending) and §Parked (T5-sad + SATD + G-1 if parked).
5. **The fuzz-absence tally** (standing signal): the list now stands at **F8, F9,
   F10 ×2, F11, and F3's sixth measurement** — present it in the log for Eugene with
   any session-G additions; re-raising Phase 0 T7 is his call, the tally is your job.
6. **Consolidation deliverables** (decided 2026-08-10; specs in
   `phase2_continue.md` §T9 — execute, don't redesign):
   - the **status preamble** at the top of `safety_refactor_plan.md` (one screen:
     state, governing decisions D-perf-4/D-seq-1, next phase, cumulative
     ledger/parked, links; history below stays untouched);
   - **plan §7.6 "Standing working rules"** — hoist the durable rules (criterion: a
     Phase 3+ session needs it verbatim; the candidate list is in the
     continuation brief's §T9), leaving phase-2-only carry-ins behind;
   - **`prompts/phase4a.md`** — the next phase's brief: the preserved D-perf-3
     protocol for `mc.rs`'s consumers first, checkpoint duties (re-measure every
     ledger row — **including the Spatial Ramps +226% datapoint session F banked:
     gradient content is the per-block-scaffolding path at its purest, the
     checkpoint's most sensitive instrument** — and re-attempt every §Parked
     family, slices-and-offsets on the table), the 4a/4b scope fence, Phase 3's
     read-first list (F4/F5/F7 read side, F2 write side) staged for 4a's exit, and
     the standing-rules pointer to §7.6.
7. **Close the phase:** Progress appendix Phase 2 marked complete with commit
   hashes; log entry whose next-action is "Phase 4a — read `prompts/phase4a.md`";
   stamp `phase2_continue.md` and this file's headers **superseded-historical**;
   commit everything; tree clean.

## 4. Gates (stated once; F3 signature as widened by session F)

Per-commit: `cargo test` both profiles, pre-existing counts frozen, ignored-20
frozen. Commit B: + one interleaved pair per bench. Family checkpoint and session
end: full `gates.sh` (`FFMPEG` set), sweeps both profiles with the R-g F3 protocol
under its **sixth-measurement signature: `mt sm=3 t∈{2,4}`, output of ANY wrong
length** (the race repacks slices, not just truncates — longer output is in-signature
now), **either profile** (the first debug hit is on record) → retry; anything
else → real, stop, revert. Session F's day-rate ran ≈1/110 under heavy machine load —
if retries pile up, run the alternating-loop comparison before drawing conclusions,
exactly as F's protocol run did. Miri per the schedule above. Ratchet non-increasing
except the R-f strangler shape (SHIM-matched `unsafe_block` rises, prose-count
wrinkles). Any gate red after a change: revert first, think second.

## 5. Non-goals

Everything in `phase2.md` §6 and the D-perf-4 prohibitions: no optimization beyond
the R-i idioms (one-hour look maximum, no boxes), no re-litigating T4/T5-sad/SATD
economics (Phase 4a owns recovery and re-attempts), no dispatch-table work (the
installers and `SWelsFuncPtrList` are Phase 4a's — including `WelsInitSampleSadFunc`),
no fixing F8/F9/F10-class arithmetic (parity, not repair), no Phase 4a early start
(finish T9's artifacts instead — the better hand-off *is* the head start), no
renumbering or plan-rewriting beyond the three specified consolidation artifacts.
