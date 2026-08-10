# Session prompt — Safety refactor, Phase 2: leaf DSP kernels, both codecs

You are executing **Phase 2** of `rust/docs/safety_refactor_plan.md`: convert every leaf
pixel/coefficient kernel — IDCT/DCT, quant, intra prediction, motion compensation, SAD,
deblocking filters, border expansion, VAA — from `(raw pointer, stride)` signatures to
safe signatures over the Phase 1 vocabulary types, behind strangler shims so **no caller
changes and no byte moves**. This is the phase with the largest mechanical volume
(~120 kernels across ~11 files) and the first phase where the §7.4 **performance budget**
is a live gate.

This prompt reflects repo state as of `956a8c07` (Phase 1 complete). Where it disagrees
with the plan, the prompt is newer; where reality disagrees with either, reality wins and
you update the plan.

---

## 1. Read first, in this order

1. `rust/docs/safety_refactor_plan.md` — §2.1, §2.2.1 (plane contracts — including the
   Phase 1 deviations recorded there), §2.2.5 (dispatch tiers, so you know what is
   *Phase 4's* job and not yours), §4 (recipes R2/R7), §Phase 2, §7 (gates, budget,
   idioms), `## Progress`.
2. `rust/docs/safety_refactor_log.md` — both entries in full. The Phase 1 entry's "Notes
   for later phases" and its **next-action paragraph are your pilot brief**: validate
   whether `PlaneCursorMut::row_mut` hoists out of 4×4 inner loops well enough for the
   budget, and whether the hottest kernels want `(buf, center, stride)` triples instead
   of the cursor.
3. `rust/docs/phase0_findings.md` (F1 — its carry-forward item 4 is a Phase 2 task;
   F3 — the sweep retry rule, now with the Phase 1 session's refined rate data) and
   `rust/docs/phase1_findings.md` (F4/F5/F7 are Phase 3 constraints — you will be near
   that code, do not "fix" them; F6 is Phase 6).
4. `rust/docs/prompts/phase0.md` §3/§6 — working agreements, unchanged: one unit per
   commit, prove-deadness/prove-claims greps pasted into commits or the log,
   revert-first on red, clean session exits.
5. The Phase 1 API you are building on: `src/safe/plane.rs` (`PaddedPlane`,
   `PlaneCursor`, `PlaneCursorMut` — `at/set/row/row_mut/advance/as_ref`, safe
   constructors from `(&[u8]/&mut [u8], center, stride)`), `src/safe/prng.rs` and the
   differential-test conventions in `tests/` (including the `scale()` Miri helper —
   reuse both).

## 2. Preconditions

- **T6 (ratchet + `gates.sh`) is REQUIRED before any conversion work.** Phase 2 is a
  long run of mechanical commits; it needs the one-command battery and the ratchet as
  per-commit instruments, and this phase produces the largest unsafe-count drops of the
  whole plan — they must be measured. Build them per `prompts/phase0.md` §T6 with the
  corrections since learned: the fn pattern is `unsafe (extern "C" )?fn` (the naive one
  missed 113 deletions); `gates.sh` prints F3's retry rule next to the sweep result,
  runs sweeps under both `RUST_ENC_PROFILE=debug` and `=release`, runs the encoder
  bench only with `FFMPEG` set (plus `BENCH_REQUIRE_FFMPEG=1`) and says SKIP loudly
  otherwise, and includes the Miri invocations the Phase 1 log records
  (`--lib` always; the differential files at phase exits).
- **T5c/T5e (decoder threading scaffolding + stragglers) — do first, recommended.**
  Small, fully teed-up in the log (step-1 grep already proven), and T5c touches
  `decoder_core.rs`, which Phase 2 also touches (expand functions) — land the deletion
  before converting in the same file. Remember the correction: **`GetThreadCount`
  becomes a literal `0`, not `1`** (`api/codec_api.rs:1831` branches on `<= 0`).
- **T7 (fuzz) stays skipped** per Eugene's direction. Consequence: there is no
  corpus-replay gate; conformance + sweeps + benches are the whole net. Note it in the
  log if a moment arises where fuzz would have caught something — that's data for
  reopening T7.
- Control run (T1 below) numbers now include Phase 1's tests: `cargo test` **375 / 0 /
  20**, `cargo test --release` **373 / 0 / 20** (the 2-test difference is `pool`'s
  `#[cfg(debug_assertions)]` tests). Sweeps 341/341 both profiles. `decode_1080p_bench`
  60/60, hashes per `perf_baseline.md`.

## 3. The shape of the work

### 3.1 The per-family conversion recipe (two commits per family)

For each kernel family below:

1. **Inventory** (in the log): every kernel fn, its exact signature, every consumer —
   which fn-pointer table slots hold it (grep the installer sites), which direct call
   sites exist, and its C++ counterpart file. Claims from this prompt about consumers
   must be re-proven by grep at conversion time.
2. **Commit A — safe kernels + differential proof, old code untouched.** Write the safe
   kernel next to the old one; add a family differential test to
   `tests/kernels_differential_phase2.rs` (PRNG inputs via the Phase 1 PRNG + `scale()`
   convention: random block contents, strides — including the minimum legal and a
   non-multiple-of-16 stride — and for plane-reading kernels, properly padded planes
   built like `AllocPicture` does). Old-vs-new must agree bit-for-bit on outputs AND on
   every written byte of the destination surface (compare whole planes, not just the
   nominal block — kernels that write past their block are exactly the bug class F1
   documented). Gates green (nothing changed yet).
3. **Commit B — swap + shim.** Old fn body becomes a shim: build views from the raw
   pointers, call the safe kernel. The now-tautological differential entries are
   **deleted in the same commit** (keep only tests that pin properties, not
   old-vs-new). Gates green — conformance/sweeps are now exercising the safe kernel
   through every real call path.
4. Ratchet after each family: counts drop, `SHIM(` count rises; both recorded.

### 3.2 Naming and shim conventions (uniform across the phase)

- The **safe kernel** gets an idiomatic snake_case name with a doc comment naming the
  C++/Wels original (`/// C++: WelsIDctT4Rec_c, codec/decoder/core/src/decode_mb_aux.cpp`).
- The **shim keeps the original Wels name and the original signature**, so zero call
  sites and zero table-installer lines change in this phase. Shim body: 3–6 lines,
  marked `// SHIM(phase2) -> safe_name`, with a `# Safety` comment stating the exact
  pointer contract it assumes (e.g. "`pPred` points at (0,0) of a 16×16 block whose
  plane has ≥1 valid row above and ≥1 valid column left; reads span
  `[-stride-1, 16*stride+16)`"). Writing these contracts down is not busywork — it is
  the documentation Phase 5 will convert callers against.
- Shims keep `unsafe extern "C"` **only** where a table typedef requires that ABI;
  plain `unsafe fn` otherwise. **Delete every `#[unsafe(no_mangle)]` from converted
  kernels** (nothing links them; the benches reach C++ via dlopen, not symbol
  interposition — and Phase 8's cdylib must not leak stray exports).
- **Do not touch the table typedefs or the installer structure** — retyping/deleting
  dispatch is Phase 4. The one exception: a table that is entirely module-internal may
  be folded now (named below: `mc.rs`'s quarter-pel table).

### 3.3 The performance protocol (new in this phase)

- Instrument: `decode_1080p_bench`, release, 3 runs, per-stream medians, against
  `perf_baseline.md` **and** against this session's control run (baselines drift with
  toolchains; the control is the truth). Encoder side: `c_vs_rust_bench` with `FFMPEG`
  set when available; otherwise record release-sweep wall time (control: ~21s) as the
  coarse encoder signal — crude, but it moves if you regress the encoder kernels badly.
- Budget (plan §7.4): investigate anything >5% on a single family before merging it;
  phase-cumulative target ≤5%. Wins are plausible (shims add a call layer, but safe
  code inlines where `Option<unsafe extern "C" fn>` never could) — record wins too.
- Safe-but-fast idioms, in preference order: fixed-size windows
  (`(&mut row[..16]).try_into().unwrap()` → `&mut [u8; 16]` hoists all checks),
  `chunks_exact`/`chunks_exact_mut` row walking, hoist `row_mut` out of inner loops,
  iterator zips for src/dst. `get_unchecked` is banned; if a kernel can't hold budget
  with the cursor API, drop to `(buf, center, stride)` triples with hoisted row slices
  — and if the *pilot* shows that's systematically needed, record the verdict in plan
  §2.2.1 so the whole phase uses one idiom, not a per-file mood.
- Word-punning replacement (taxonomy T7): `u32::from_ne_bytes`/`to_ne_bytes` over slice
  windows for the `LD32/ST32` idiom; value-level tricks (`0x01010101u32.wrapping_mul(v)`)
  stay as-is — they were never a memory operation. Confirm codegen parity via the bench,
  not by staring at assembly first.

## 4. Tasks — families in order

Sized at 5–7 sessions. Suggested split: A = preconditions + T1 + pilot (+retro);
B = decoder intra pred; C = mc + sad + intra_pred_common; D = deblocking + F1 cleanup +
expand; E = the four encoder files; F = processing + cleanup sweep + phase exit. Strict
order within the list; clean stops anywhere.

### T1 — Control run
Full battery via `gates.sh`. Numbers into the log. This is also the perf control
(3-run bench medians, sweep wall time).

### T2 — PILOT: `decoder/decode_mb_aux.rs`
The smallest family (374 LOC, ~13 kernels: 4×4/8×8 IDCT + dequant + luma/chroma
recon-add, fixed shapes, zero derefs) converted **deliberately first to judge the API**,
per the Phase 1 hand-off. Consumers: the `pIdctResAddPredFunc*` /
`sBlockFunc`-adjacent slots installed in `decoder_core.rs` and direct calls in
`decode_slice.rs`/`error_concealment.rs` — inventory precisely.

**Pilot retro (its own log section, before any other family):** cursor vs triple for
hot signatures; `row_mut` hoisting vs the budget (bench numbers); shim ergonomics
(anything the `# Safety` contract couldn't express cleanly?); differential-test cost
per kernel. Record verdicts in plan §2.2.1/§Phase 2. **If the API needs a change, make
it now** — in `src/safe/`, with Phase 1's own tests updated — before 100 more kernels
consume it.

### T3 — `decoder/get_intra_predictor.rs` — the perf worst case, taken early
44 kernels, uniform `(pPred: *mut u8, kiStride: i32)`, highest pointer-arithmetic
density in the tree, ~140 punned u32 accesses; installed into the four
`pGetI*PredFunc` table families (`decoder_core.rs:1942+`). All 44 read the `-1`
column / `-stride` row and write the block — `PlaneCursorMut` was designed for exactly
this; the shim contract is identical for the whole file. Bench after conversion; this
family decides whether the phase's idiom holds. **Trap:** these are 2-arg kernels;
`common/intra_pred_common.rs` has same-named 3-arg cousins — different functions, never
unify (T5 below).

### T4 — `common/mc.rs` — motion compensation
26 kernels + `InitMcFunc`; the 6-tap Wiener filters; consumers are `SMcFunc` slots used
by decoder MC/EC and encoder MD/ME. Two structural notes: fold the module-internal
`pWelsMcFunc_c: [[fn; 4]; 4]` quarter-pel table into a `match` (or a table of **safe**
fn pointers — either is fine, it never leaves the module); MC kernels read *source*
planes at MV-derived offsets whose legality comes from the caller's clamp
(`decode_slice.rs:1072-1091`) — the shim contract must state the clamp invariant
verbatim, and the safe kernels take a `PlaneCursor` (read) + `PlaneCursorMut` (write)
pair. Bench (MC is the other decode-hot path).

### T5 — `common/sad_common.rs` + `common/intra_pred_common.rs`
- SAD: 7 single-block + 7 four-point kernels. **The `Four` kernels read outside the
  nominal block** (`-stride`, `-1`, `+1`) — they must take a plane cursor, not a block
  slice (the survey flagged this; the differential test must cover the edge reads).
  The three pre-existing safe wrappers (`sample_sad_4x4/8x8/16x16`, unused) get
  absorbed/superseded — fold them into the new safe kernels rather than keeping two
  safe SAD APIs. Consumers: `encoder/sample.rs` table installers +
  `processing/scene_change_detection.rs:103` direct call.
- intra_pred_common: post-T5b this is just the two 16×16 V/H `_c` kernels (3-arg:
  `(pPred, pRef, stride)`) + typedefs. Quick.

### T6 — `common/deblocking_common.rs` + the F1 cleanup + expansion
- Deblocking: 6 inner kernels where **the (iStrideX, iStrideY) swap encodes
  vertical-vs-horizontal** — port that trick honestly: safe kernels take
  `(cursor, step_x: isize, step_y: isize)` and index `center + i*step_x + j*step_y`
  through checked math (a `-3*step` read reach; contract in the shim). The 14 ABI
  wrappers that fix the stride pair collapse onto the safe kernels; `WelsNonZeroCount_c`
  and `DeblockingInit` convert trivially. Consumer trap: the decoder installs this
  module's table via an untyped double-cast (`decoder_core.rs:1927`) — leave the cast,
  it's Phase 4's.
- **F1 carry-forward (in scope, explicitly):** in `encoder/deblocking.rs`, give the
  boundary-strength scratch its real type end-to-end — `uiBS: [[[u8; 4]; 4]; 2]`
  through `DeblockingBSCalc*`/`DeblockingInterMb` signatures — and **delete the five
  `from_raw_parts_mut(uiBS as *mut u8, 32)` sites**, which are currently the only
  thing enforcing the 16-vs-32-byte relationship that caused the release segfault.
  Check whether any of these fns sit in table slots (then: shim) or are direct-called
  (then: change signatures directly — the callers are in the same file). This is the
  one Phase 2 task inside a Phase-6 file; keep the diff surgical.
- `common/expand_pic.rs` + the expand functions in `decoder_core.rs` (border
  expansion, writes the padding itself, negative-y row writes). Safe shape: free
  functions over `(&mut [u8], origin, stride, w, h)` — full-allocation slice, not a
  mid-pointer (plan §2.2.1's "PaddedPlane methods" packaging happens in Phase 5 when
  `Picture` owns planes; update the plan wording when you commit). **This family has
  the trickiest shim in the phase:** callers hand a mid-pointer `pDst`; the shim must
  reconstruct the allocation span using the padding constant of its call site
  (32 luma / 16 chroma — per-variant, from `PADDING_LENGTH`), i.e.
  `from_raw_parts_mut(pDst.sub(pad*stride + pad), (h + 2*pad) * stride)`. Write that
  contract out per shim; if any call site's pad cannot be named confidently, give that
  shim an explicit pad parameter and make the call site say it. The two
  `mem::transmute` re-wraps of expand fn-pointers (`decoder_core.rs:933-943`) stay —
  Phase 4.

### T7 — The four encoder kernel files
`encoder/encode_mb_aux.rs` (DCT/quant/scan/copy on fixed blocks —
`(&mut [i16; 16], &[i16; 8], &[i16; 8])`-style signatures; post-T5b this file is pure
`_c` kernels), `encoder/decode_mb_aux.rs` (recon IDCT), `encoder/sample.rs` (SAD/SATD +
the `pfSampleSad`/`pfSample4Sad` installers at `sample.rs:218-241` — installers keep
installing shims), `encoder/get_intra_predictor.rs` (encoder intra kernels + the
installer that wires `common/intra_pred_common`'s pair). Encoder perf signal: sweep
wall time + `FFMPEG` bench if available.

### T8 — Processing kernels
`processing/vaacalc.rs` (5 leaf kernels `(cur, ref, w, h, stride, out-params…)` — safe
signatures take `&[u8]` planes + `&mut` out-slices; this file already has 2 real unit
tests, extend them) and `processing/adaptive_quantization.rs`
(`SampleVariance16x16_c`). The `IWelsVP` plumbing above them is Phase 4 — kernels only.

### T9 — Phase exit
1. Sweep for stragglers: `grep -rn "SHIM(phase2)" src/ | wc -l` recorded;
   `grep -rn "no_mangle" src/` — kernels must contribute zero (the api/ factories and
   version fns remain, they're the drop-in ABI).
2. Full battery + Miri protocol (`--lib`; the differential file under Miri with
   `scale()`; expect it to execute remaining old code — findings per the F-doc format
   if it flags anything).
3. Perf: final 3-run medians vs control + baseline; per-family deltas table into the
   log; `perf_baseline.md` gets a Phase 2 column **added** (never overwrite the
   Phase 0 anchor numbers).
4. Bookkeeping per convention: Progress appendix "### Phase 2" checklist with hashes;
   plan updates (pilot verdicts in §2.2.1, expand-packaging wording, anything else
   reality corrected); log entry with next-action ("Phase 3, decoder read side first —
   and read F4/F5/F7 before touching it"); auto-memory update.

## 5. Gates

- **Per-commit:** `cargo test` + `cargo test --release` — the pre-existing counts are
  frozen (375/373 will grow in commit A of each family and shrink back in commit B as
  differential entries are deleted; the *original* suites and ignored-20 never move).
- **Per-family (commit B) and checkpoints:** + ratchet `check` (drops recorded), +
  `decode_1080p_bench` when the family is on the decode path (T2, T3, T4, T6-decoder
  parts), + sweeps `st mt def` both profiles when on the encode path (T6-F1, T7, T8).
- **Session end:** full `gates.sh` battery + perf medians.
- **F3 retry rule, as it now stands:** an `mt` failure at `sm=3`, `t` in {2,4}
  (zero-byte or short output) → re-run that config; if it clears, it's F3. **This
  applies in *both* profiles.** Debug's old exemption is gone: it was an artefact of
  the driver building at `opt-level = 0`, too slow to lose the race, and the driver is
  now `opt-level = 3` with the checks still on (F3's fourth measurement). When the rate
  looks elevated (more than one hit in a session), **prove it by alternating HEAD and
  the control commit inside one loop** — sampling them at different times is not a
  comparison for a load-sensitive race — and append the measurement to F3. A failure at
  any other config, or in `st`/`def`, in either profile: real, stop, revert,
  investigate.
- Byte-exactness: any conformance hash change, frame-count change, sweep byte
  difference, or `#[ignore]`-set change is a hard stop. Frame counts before hashes
  when diagnosing.

## 6. Explicit non-goals for Phase 2

Fn-pointer **table types, installers' structure, and call-site dispatch** — Phase 4
(only `mc.rs`'s internal quarter-pel table is yours). Bitstream/CABAC readers-writers
and anything F4/F5/F7 touch — Phase 3 (the findings are pinned by tests; do not
"clean up" the quirks). Drivers and MB-level logic (`decode_slice.rs`,
`decoder/deblocking.rs` driver logic, `encoder/deblocking.rs` beyond the named F1
surgical fix, MD/ME/mode-decision) — Phases 5/6. `Picture`/DPB/allocators — Phases
5/6. No `Cargo.toml` changes, no renames of existing callers, no fixing F2/F3/F4/F5/F6/F7
(F1's uiBS cleanup is the sole findings-derived fix, and it is named). No Phase 3 early
start; surplus time goes to sharper shim contracts, more differential edge coverage
(odd strides, minimum pads, boundary MVs), and the pilot-retro documentation — Phase 5
converts callers against those contracts, and their quality is this phase's real
deliverable beyond the ratchet drop.
