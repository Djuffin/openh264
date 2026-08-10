# Session prompt — Safety refactor, finishing Phase 2 (T7 part 1 → T7 part 2 → T9)

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

State at `915bb554` (tree clean; the last two commits are docs):

| | |
|---|---|
| Tests | 395 debug / 393 release / 20 ignored |
| Sweeps | 341/341 both profiles (~25s debug, ~21s release), F3 retry rule in force |
| Ratchet | `unsafe_fn` 1360, `raw_ptr` 5198, `SHIM(` 97, `no_mangle` 24 — regenerated at T6 expand-B; run `check`, never trust remembered numbers |
| Cumulative ledger | decode ≈ **+17 / +10 / +10%**, encoder ≈ **+11%** — tripwire headroom is real but not infinite |
| Parked | T5-sad (14 kernels, raw live, re-attempt at Phase 4a) |

**Counts corrected while writing this brief** (the estimate-drift rule struck again):
`encoder/get_intra_predictor.rs` has **27** `pub unsafe fn` definitions, not the 33
every earlier doc says; `sample.rs`'s "8" is **7 SATD kernels + 1 installer**. Recount
each file at session start anyway; the counts below are grounded but not gospel.

---

## 1. Read first

1. This file, then `prompts/phase2_continue.md` §2 (rules R-a…R-p — R-e, R-l, R-m,
   R-n and the golden-run/F10 additions are the ones this stretch leans on).
2. `rust/docs/safety_refactor_log.md` — sessions D and E entries in full (T8's
   kernel-shape lessons and instrument rules; T6's probe lesson and per-session
   noise floor; both sessions' recorded order-deviations — you inherit the same
   temptation, and the brief's order is binding).
3. `rust/docs/phase2_findings.md` — **F8** (i16 IDCT overflow: the arithmetic-parity
   precedent) and **F10** (parked raw kernels' trailing bump is UB on exact-span
   buffers — it dictates your differential harness sizing).
4. `perf_baseline.md` — §Ledger and §Parked (you will add rows to both), §Phase 2 T8
   (the exact-span trim and the two microbench lies), the T4 row-walker table
   (negative results you must not re-propose).
5. Plan §5 Phase 4a and §7.4 D-perf-4/D-seq-1 — so T9's hand-off artifacts say the
   right things.

## 2. Session F — T7 part 1: `encode_mb_aux.rs`, `encoder/decode_mb_aux.rs`, `sample.rs`

Open with: commit any inherited doc changes; control battery (`FFMPEG` set); ratchet
`check`; **fresh null run** (the floor is per-session — session D read ±1.8%, session
E ±5.4% on tiny rows; yours is whatever the null says today).

### F-1. `encoder/encode_mb_aux.rs` — 22 kernels, the forward-transform family

- **What's there:** forward DCT (`WelsDctT4_c`, `WelsDctFourT4_c`), quantization
  (`WelsQuant4x4_c` family, `WelsQuantFour4x4Max_c` etc.), scan/zig-zag
  (`WelsScan4x4Ac_c` and friends), `WelsGetNoneZeroCount_c`, hadamard/DC transforms,
  and copy helpers. Every block dimension is a compile-time constant — signatures
  convert to fixed arrays (`&mut [i16; 16]`, `&[i16; 8]`), which makes the shim spans
  **derivable from the signature** (T2's situation, not T3's): expect short contracts
  and no span-helper subtleties. Phase 0 already deleted this file's 80 SIMD stubs
  and its `no_mangle`s — what remains is all real.
- **R-e, checked per kernel before writing:** the forward DCT sums differences of
  `u8` pixels in `i16` (max |diff| 255 × gains through the butterfly — do the bound
  arithmetic like F8 did); quant multiplies into `i32` with rounding offsets — match
  the old Rust's widths and operations exactly, wrapping only where it wraps, and
  bound differential inputs to the in-contract range. Any newly *noticed*
  overflow-capable intermediate → F-finding in F8's format, nothing else.
- **Dispatch:** these install into `SWelsFuncPtrList` slots (`pfQuantization*`,
  `pfDctT4*`, `pfScan4x4*`, `pfTransformHadamard4x4Dc`, …) — installers keep
  installing the shims; some helpers are also direct-called — inventory call paths
  first (`grep -n` each name) so commit B swaps every path.
- **Differential + probes:** exhaustive over the quant max/rounding branches
  (selector flags per R-d), random anchors, whole-destination comparison; every span
  probe carries the **golden direct run** (session E's rule — exact-span + touch-set
  alone is blind along uniform axes).
- **Commit B measurement:** one interleaved pair, both benches. This family is
  encoder-per-block-dense (transform+quant runs for every coded block) — a T4-shaped
  per-call number is plausible. Tripwire arithmetic against encoder +11%: park only
  if a stream's cumulative projects past +25% median; otherwise ledger and continue,
  flagging >15% single-family. One profile+disassembly look maximum if it's ugly.

### F-2. `encoder/decode_mb_aux.rs` — 6 kernels, the encoder's recon IDCT

The decoder pilot's shape replayed: `IdctResAddPred_c` and friends, fixed 4×4/8×8
blocks, plane cursor for the recon write. **This is F8's arithmetic class exactly**
(i16 IDCT intermediates) on the encoder side — same width-parity discipline, same
bounded differential inputs. Small, fast, should be the cleanest family of the
session. Same commit-B pair; expect noise-level.

### F-3. `sample.rs` — 7 SATD kernels: **prove-and-park, directly. No commit B.**

- The seven `WelsSampleSatd{4x4,8x4,4x8,8x8,16x8,8x16,16x16}_c` are ME's cost
  kernels — the same tiny-block, call-dense profile that parked T5-sad, with a
  Hadamard butterfly on top. D-perf-2's measurements are the tripwire projection
  (sad-class swap cost +16.8% median onto +11% cumulative crosses +25%), so:
  **commit A lands safe kernels + differentials + span probes; nothing installs
  them.** Their §Parked row lists re-attempt = Phase 4a, alongside T5-sad.
- **F10 rule from the first differential:** raw-side buffers sized **whole rows** —
  the raw kernels' post-last-row pointer bump is UB on exact-span buffers and Miri
  runs this file at T9. Exact spans on the safe side only.
- **R-e:** the C++ SATD accumulates the butterfly in `i32` after `i16` stages —
  verify against the old Rust per kernel.
- `WelsInitSampleSadFunc` (the installer) is **untouched**: it installs the parked
  raw `sad_common` names and would install SATD — both stay raw until Phase 4a;
  the installer itself is dispatch plumbing (Phase 4a's to delete).
- Do **not** try D-perf-2's slices-and-offsets idea here — it's queued for Phase 4a's
  checkpoint, where the convention change can be made once for all parked families.

Close session F per the standing exit protocol: log entry (gates control vs final,
per-family verdicts, ledger/parked updates with evidence), Progress appendix,
ratchet regenerated with reason, hand-off naming session G's first action.

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
  encoder, so let the pair decide: cumulative past +25% median on any stream →
  park with the SATDs (Phase 4a re-attempt); under → swap and ledger. Either way
  this is the phase's last swap decision — record it with its arithmetic shown.

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
5. **The fuzz-absence tally** (standing signal): list every finding a fuzzer would
   plausibly have reached first (F8, F10, any new ones) in the log for Eugene —
   re-raising Phase 0 T7 is his call, the tally is your job.
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
     ledger row, re-attempt every §Parked family, slices-and-offsets on the table),
     the 4a/4b scope fence, Phase 3's read-first list (F4/F5/F7 read side, F2 write
     side) staged for 4a's exit, and the standing-rules pointer to §7.6.
7. **Close the phase:** Progress appendix Phase 2 marked complete with commit
   hashes; log entry whose next-action is "Phase 4a — read `prompts/phase4a.md`";
   stamp `phase2_continue.md` and this file's headers **superseded-historical**;
   commit everything; tree clean.

## 4. Gates (unchanged, stated once)

Per-commit: `cargo test` both profiles, pre-existing counts frozen, ignored-20
frozen. Commit B: + one interleaved pair per bench. Family checkpoint and session
end: full `gates.sh` (`FFMPEG` set), sweeps both profiles with the R-g F3 protocol
(signature `mt sm=3 t∈{2,4}` short-or-zero output **either profile** → retry;
anything else → real, stop, revert). Miri per the schedule above. Ratchet
non-increasing except the R-f strangler shape (SHIM-matched `unsafe_block` rises,
prose-count wrinkles). Any gate red after a change: revert first, think second.

## 5. Non-goals

Everything in `phase2.md` §6 and the D-perf-4 prohibitions: no optimization beyond
the R-i idioms (one-hour look maximum, no boxes), no re-litigating T4/T5-sad/SATD
economics (Phase 4a owns recovery and re-attempts), no dispatch-table work (the
installers and `SWelsFuncPtrList` are Phase 4a's — including `WelsInitSampleSadFunc`),
no fixing F8/F9/F10-class arithmetic (parity, not repair), no Phase 4a early start
(finish T9's artifacts instead — the better hand-off *is* the head start), no
renumbering or plan-rewriting beyond the three specified consolidation artifacts.
