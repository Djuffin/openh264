# Session prompt — Safety refactor, Phase 2 continuation (T4 → T9)

You are continuing **Phase 2** of `rust/docs/safety_refactor_plan.md`.
**`prompts/phase2.md` remains the governing brief** — recipe (§3.1 two commits per
family), naming/shim conventions (§3.2), perf protocol (§3.3), gates (§5), non-goals
(§6) all apply unchanged. This file is the continuation state: what's done, what the
first sessions learned that you inherit as *rules*, and the remaining task list with
its carry-ins. Where this file and `phase2.md` disagree, this file is newer.

State as of **`3ddd2405`** (tree clean, full battery green):

| | |
|---|---|
| Done | Preconditions (Phase 0 complete except T7-fuzz, deferred by Eugene), T1 control, T2 pilot (4 kernels, −1.3…1.8% — a win), T3 (42 kernels, ±1% wash), **T4 `common/mc.rs` (26 kernels — landed over budget, investigated, and carried under decision D-perf-1: plan §7.4)** |
| Remaining | T5 `sad_common`+`intra_pred_common` (16 fns) → T6 `deblocking_common`+F1+expand (~24) → T7 four encoder files (**69** — the biggest task, larger than T3+T4 combined) → T8 processing (11) → T9 exit. **~120 fns of the phase's real ≈190** (the plan's "~120 total" was ~60% low; count files before sizing sessions) |
| Instruments | `rust/tools/gates.sh` + ratchet; gates.sh prints the F3 retry rule and a T7-SKIP line every run; the **interleaved-pairs perf protocol and the scratch microbenchmark are now normative** (§7.4, R-h) |
| Counts | tests 378 debug / 376 release / 20 ignored (the +1 over T4's control is its surviving span property test); sweeps 341/341 both profiles; ratchet baseline regenerated at `ea52b387` — run `check`, don't trust remembered numbers |

Suggested split of the remaining 3–4 sessions: **S1 = T5 + T8** (both small and
mechanical — T8 is pulled forward deliberately to fill the session while keeping T6
isolated); **S2 = T6 alone** (the likeliest overrun — don't pair it with anything);
**S3 = T7 part 1** (`encode_mb_aux` 22 + `encoder/decode_mb_aux` 6 + `sample` 8);
**S4 = T7 part 2** (`encoder/get_intra_predictor` 33) **+ T9**. Clean stops anywhere,
per the standing exit protocol.

---

## 1. Read first

1. `rust/docs/safety_refactor_log.md` — **both** Phase 2 entries (sessions A and B) in
   full: the pilot retro verdicts, T3's carry-forwards, and session B's T4 write-up
   (the budget investigation, the span property test, the "four things the remaining
   families inherit"). This is the highest-density context you have.
2. `rust/docs/phase2_findings.md` — **F8** (the 8×8 IDCT's `i16` intermediates overflow;
   debug panics where the C++ wraps; pre-existing, open, unreachable on conformant
   streams). It generalizes — see rule R-e below.
3. `rust/docs/prompts/phase2.md` — re-skim §3 (recipe/conventions/perf) and your
   families' entries in §4; the per-family traps written there still hold.
4. `perf_baseline.md` — the Phase 2 T2/T3 rows are your evidence base; Phase 0 anchors
   are never overwritten.
5. Plan `## Progress` appendix — check off against reality before starting.

## 2. Rules the first sessions established (inherit, don't re-derive)

- **R-a — The perf mechanism, and why the budget is holding:** bounds checks land
  **per row, not per sample** (one `row_mut` per output row amortizes over the row's
  arithmetic), and `copy_from_slice`/`fill` on fixed windows compile to the same wide
  stores the punned `u32`/`u64` accesses existed for. Structure every remaining kernel
  this way first; measure second. §9's decoder-side perf risk is downgraded on T3's
  evidence — **the encoder side has had no measurement yet**; T4 (shared consumer) and
  T7 (encoder-only) are where that claim gets tested.
- **R-b — Punned accesses are almost always byte *moves*.** In 140 T3 accesses,
  `from_ne_bytes` was needed **zero** times — `fill(v)` and `copy_from_slice` covered
  everything. Check for genuine arithmetic use before reaching for `from_ne_bytes`;
  value-level tricks (`0x01010101u32.wrapping_mul(v)`) stay as-is.
- **R-c — The shim-span helper is the pattern.** T3's shims aren't derivable from the
  signature alone; one helper computes the slice span (`centre`, `len`, anchor,
  `top_reach`) and *nothing else does that arithmetic*. Reuse the helper approach per
  family; the contract sentence naming **why** the negative reach is legal
  (`PADDING_LENGTH`, or the caller's MV clamp) is what Phase 5 converts callers
  against — that sentence is the deliverable.
- **R-d — Differential-test discipline:** sweep availability/selector flags
  **exhaustively** (T3 found kernels that ignore a flag they take — a random sweep
  blurs exactly what a conversion gets wrong); anchor test blocks at **random legal
  offsets** (kernels written around unaligned stores must be exercised unaligned);
  compare **every written byte of the destination surface**, not the nominal block.
- **R-e — Arithmetic parity, the F8 rule (new, binding for T4/T7 especially):** when a
  kernel's intermediates can overflow, the safe kernel reproduces the **old Rust
  port's exact behavior** — same integer widths, same operations, which means the same
  debug-panic exposure the old code already has and the same release wrapping. Never
  widen, never add `wrapping_*` the old code lacked, never "fix" it — that is a
  behavior change on malformed input and it belongs to P13, later. Newly *noticed*
  overflow-capable intermediates get an F-finding (F8's format) and nothing else.
  Expect candidates in T4's 6-tap filter sums and T7's DCT/quant/SATD arithmetic —
  check the widths against both the old Rust and the C++ before writing the safe body,
  and bound differential-test inputs to the in-contract range where the old code
  doesn't panic (F7/F8 precedent).
- **R-f — Ratchet arithmetic during the strangler phase: judge the shape, not the
  sign** (corrected by T4 — the earlier "raw_ptr strictly non-increasing" version of
  this rule was wrong). `raw_ptr` counts *occurrences* — signatures and casts, not
  body arithmetic — so a shim that keeps its raw signature keeps its count, and a
  shared helper that turns a pointer into a slice may legitimately add one or two
  (T4 went **+2**, both from `shim_wh`; paying that beat 29 copies of the span
  arithmetic). `SHIM(` rises only in a family's commit B; `unsafe_block` may rise
  only by that commit's shim count; `no_mangle` non-increasing; `unsafe_fn` ~flat
  until Phase 5. Two counting traps from T4: **never fold shims into a macro** — it
  makes `unsafe_fn` report a drop that didn't happen and hides `SHIM(` markers; write
  them out one per definition, marker on each.
- **R-g — F3 protocol, as practiced:** one release-`mt` hit at `t=4 sm=3` → re-run
  rather than argue, even when the change is structurally unrelated. More than one hit
  in a session → equal-count sweeps at HEAD vs the control commit, and append the
  measurement to F3. Any other config, any debug hit, any `st`/`def` hit: real, stop,
  revert, investigate. And log any moment where the missing fuzzer (T7-skip) would
  plausibly have caught something first — F8 is the first such entry; accumulation is
  the signal to re-raise T7 with Eugene.
- **R-h — The perf protocol is interleaved pairs plus an early microbenchmark**
  (normative, §7.4). Sequential runs of the same binary drift ~3% — an unpaired
  reading of T4 said +5.1% when the paired reading said +8.7%, and the session acted
  on the wrong one for a while. Keep both binaries on disk, interleave in one loop,
  3+ pairs, medians. For any family on a hot path, **build the scratch
  microbenchmark first** (old kernels via `git show` vs new, per phase and block
  shape) — T4's brief says "first in T5 and T7, not third"; it is what separates
  kernel cost from scaffolding cost, and that separation is what §7.4's two ledgers
  are judged on. Profile with `/usr/bin/sample` before theorizing — T4's first two
  theories were wrong and cost a build-bench cycle each.
- **R-i — The perf defect catalog from T4** (check each before measuring, they all
  generalize): `copy_from_slice` on a runtime length is a `memmove` **call** — use
  const-generic widths (`copy_rows::<16>`) exactly as the C++ dispatches fixed
  widths; indexing N same-length slices by `j` leaves N bounds facts per sample —
  zip the iterators, and roll the row window instead of re-fetching; match the C++'s
  inline structure (`#[inline(always)]` inner kernels, `#[inline(never)]` on
  composites carrying big scratch). And the negative results are binding: by-value
  cursors (wash), dispatch-over-shims (wash), and `chunks()`-based row walkers
  (**worse everywhere**, the API was built, measured, and removed — read
  `perf_baseline.md` §Phase 2 T4 before re-proposing any of the three).

## 3. Remaining tasks

### T4 — `common/mc.rs` — DONE (`46053953`…`3ddd2405`), carried over budget under D-perf-1
Landed at +8.2/+7.2/+7.0% on the decode streams; kernel bodies at 0.88–0.99x; the
residual is fixed per-call shim overhead, ledgered in `perf_baseline.md` §Ledger,
bounded by §7.4's ≤10% ceiling, checkpointed at Phase 4, cleared at Phase 5. **Further
T4 optimization is closed** — three structural fixes were built, paired-measured, and
rejected (R-i). If a later family's work makes a T4 stream drift further, that counts
against the ceiling, not against T4's closed investigation. Everything reusable from
the session is in R-f/R-h/R-i and the log's "four things the remaining families
inherit" (the 17-wide encoder half-pel ceiling; clamp-as-contract; per-kernel reach
tables, not unions; the span property test that replaced per-kernel equivalence
entries — copy that test shape in every remaining family).

### T5 — `common/sad_common.rs` + `common/intra_pred_common.rs` (next action; unblocked by D-perf-1)
16 fns (14 + 2 — the one brief estimate that has matched reality; still recount).
Per `phase2.md` §T5 unchanged: the `Four` SAD kernels read **outside the nominal
block** (`-stride`, `-1`, `+1`) — plane cursors, not block slices, and the
differential must cover the edge reads; absorb the three pre-existing unused
`sample_sad_*` safe wrappers into the new kernels (one safe SAD API, not two);
consumers are `encoder/sample.rs`'s installers + one direct call in
`processing/scene_change_detection.rs:103`. intra_pred_common is two 3-arg kernels —
mind the same-name-different-arity collision with the decoder file you already
converted. **Build the microbenchmark first (R-h)**: SAD is encoder-ME-hot and
call-dense — the same per-call-overhead profile that put T4 over budget — so this is
the first family where the two-ledger split gets applied from the start rather than
discovered late. Per-kernel reach tables per T4's carry-in, not a union span.

### T6 — `deblocking_common` + the F1 surgery + expansion (schedule alone)
The phase's two hardest items live here; both have full briefs in `phase2.md` §T6 —
follow them. Compressed reminders: deblocking's `(iStrideX, iStrideY)` swap **is** the
V/H encoding — port as explicit `(step_x, step_y)` with the `-3*step` reach in the
contract; the decoder's untyped double-cast installer stays (Phase 4). The **F1 uiBS
surgery** (`encoder/deblocking.rs`): real type `[[[u8; 4]; 4]; 2]` end-to-end, the five
`from_raw_parts_mut(…, 32)` sites deleted, table-membership checked first, diff kept
surgical — it's the one Phase-2 task inside a Phase-6 file. The **expand shims** must
reconstruct the full allocation span from a mid-pointer with the per-variant pad
constant (32 luma / 16 chroma), explicit pad parameter where a call site can't prove
it; the two `mem::transmute` fn-pointer re-wraps stay (Phase 4). R-c applies doubly
here: the expand span helper and its contract sentence are the whole game.

### T7 — the four encoder kernel files (first real encoder perf signal; plan for two sessions)
**69 fns by real count** — `encode_mb_aux.rs` 22, `encoder/decode_mb_aux.rs` 6,
`sample.rs` 8 (installers keep installing shims), `encoder/get_intra_predictor.rs` 33
— the biggest task in the phase, larger than T3 and T4 combined; the suggested split
is S3 = the first three files, S4 = `get_intra_predictor` + T9. This is where R-a's
mechanism gets its encoder-side test and where R-e is most likely to bite
(DCT/quant/SATD intermediates — check widths against both the old Rust and the C++
before writing each safe body). **Microbenchmark first (R-h), per family** — there is
no decode-bench equivalent on this path, so the microbench plus `FFMPEG`-gated
`c_vs_rust_bench` (1080p Mandelbrot as primary, per `perf_baseline.md`) *are* the
instruments, with sweep wall time as the coarse cross-check; if a regression shows up,
bisect by family — that's why the commits are two-per-family. SAD/SATD in `sample.rs`
have T4's call-density profile: expect scaffolding overhead, apply the two-ledger
split, and ledger any deficit with its evidence rather than burning the session
optimizing closed ground.

### T8 — `processing/{vaacalc,adaptive_quantization}.rs` kernels
Per `phase2.md` §T8: safe signatures with `&[u8]` planes + `&mut` out-slices; extend
the file's two real unit tests; the `IWelsVP` plumbing above is Phase 4's.

### T9 — Phase exit
Per `phase2.md` §T9, plus what's now known: final `SHIM(phase2)` and `no_mangle`
counts recorded (kernels must contribute zero no_mangles; api/ exports remain); full
battery + the Miri protocol (`--lib` + differential files with `scale()`); final
**interleaved-pair** medians (R-h, not the old 3-run protocol) and the per-family
delta table; **append** a Phase 2 column to `perf_baseline.md` and reconcile the
§Ledger (T4's entry stays open with its Phase 4 checkpoint pending; any new entries
carry their evidence); Progress appendix updated with hashes. Two T9-specific items
from session B: **evaluate widening the Miri gate to the port's own unit tests** —
`gates.sh` runs Miri only on `--lib safe::`, which is why a port unit test exercising
UB (the mc alias test reading `pSrc[-2]` off a bare array) went unseen until a shim
materialized the span; a gate change is legitimate at a phase boundary and this is
the boundary. And carry the straggler grep for any arch-suffixed or table-installed
stub the per-family passes missed. Log entry's next-action: "Phase 3, decoder read
side first — read F4/F5/F7 before touching anything, and F2 before the write side";
auto-memory updated (Phase 2 complete, where the shims stand, ledger state, ratchet
shape).

## 4. Gates

Unchanged from `phase2.md` §5, with the R-f ratchet shape and R-g F3 protocol from
this file as the operative refinements. Frozen invariants: the original test suites'
counts, ignored-20, 341/341 both profiles, conformance hashes and frame counts, the
`#[ignore]` set. Per-family bench when on the decode path; sweep timing + `FFMPEG`
bench when on the encode path; session-end full `gates.sh`.

## 5. Non-goals

Everything `phase2.md` §6 lists, plus: no fixing **F8** (R-e is parity, not repair),
no reopening T7-fuzz without Eugene (log the signal instead), no dispatch-table work
beyond the two named module-internal exceptions (mc's quarter-pel fold; the F1
signature surgery), no Phase 3 early start. Surplus time: sharpen shim contracts and
differential edge coverage — T6's contracts especially, since they're the ones Phase 5
will lean on hardest.
