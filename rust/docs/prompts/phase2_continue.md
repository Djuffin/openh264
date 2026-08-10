# Session prompt — Safety refactor, Phase 2 continuation (T4 → T9)

You are continuing **Phase 2** of `rust/docs/safety_refactor_plan.md`.
**`prompts/phase2.md` remains the governing brief** — recipe (§3.1 two commits per
family), naming/shim conventions (§3.2), perf protocol (§3.3), gates (§5), non-goals
(§6) all apply unchanged. This file is the continuation state: what's done, what the
first sessions learned that you inherit as *rules*, and the remaining task list with
its carry-ins. Where this file and `phase2.md` disagree, this file is newer.

State as of **session D's final commit** (tree clean, full battery green):

| | |
|---|---|
| Done | Preconditions (Phase 0 complete except T7-fuzz), T1 control, T2 pilot, T3 (42 kernels), **T4 `common/mc.rs` (26, over budget — see D-perf-3)**, **T5-intra (2 kernels, +0.57%)**, **T8 `processing/*` (6 kernels, +3.5% encoder)** |
| Parked | **T5-sad `common/sad_common.rs` (14 kernels).** Proven, uninstalled, raw kernels run. **D-perf-2's one attempt was spent and the verdict is park** — T7's SAD/SATD go prove-and-park from the start. Box closed; do not reopen in this phase |
| **OPEN — needs Eugene** | **D-perf-3: the encoder-side ceiling is breached.** T4 costs **+7.6% median / +16.7% worst** on the encoder (isolated this session), T8 adds +3.5% on top, cumulative ≈ **+11%** against a 10%-per-stream hard ceiling. Three options in `perf_baseline.md` §"The encoder-side ceiling is breached". **Put this to Eugene before starting T6** |
| Remaining | **T6 (alone — the phase's hardest conversion)** → T7 four encoder files (69, SAD/SATD prove-and-park) → T9 exit |
| Instruments | `gates.sh`; **always `FFMPEG=/opt/homebrew/bin/ffmpeg bash rust/tools/gates.sh`**. Interleaved pairs (R-h), L1-vs-streaming working set (R-j), **the null run (R-l)** and **post-swap raw recovery (R-m)** are all normative |
| Counts | tests 392 debug / 390 release / 20 ignored; sweeps 341/341 both profiles; ratchet baseline regenerated at T8 B — run `check`, don't trust remembered numbers |

**Session order for the next session:**
1. **Put D-perf-3 to Eugene** before touching code. The tree is over the encoder
   ceiling *now*, and T6 adds shims to both codecs.
2. **T6 alone**, per `phase2.md` §T6 and §3 below. Never pair it.

Then **S+1 = T7 part 1** (`encode_mb_aux` 22 + `encoder/decode_mb_aux` 6 + `sample` 8,
the last prove-and-park); **S+2 = T7 part 2** (`encoder/get_intra_predictor` 33) **+
T9**. Clean stops anywhere, per the standing exit protocol.

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
- **R-g — F3 protocol, broadened by session C's measurement:** the signature is now
  **release `mt`, `sm=3`, `t∈{2,4}`, output short *or* zero-length** — the original
  "t=4, zero bytes" rule was too narrow and would have misread session C's hits as a
  regression. One hit → re-run rather than argue, even when the change is structurally
  unrelated. More than one hit in a session → **alternate both trees in one loop** and
  compare counts; sequential sampling actively misleads (it read "HEAD 4/8 vs control
  0/6" where the alternating loop read control 4, HEAD 1 — the same instrument lesson
  as R-h, applied to failure rates). Append every such measurement to F3. Any other
  config, any debug hit, any `st`/`def` hit: real, stop, revert, investigate. And log
  any moment where the missing fuzzer (T7-skip) would plausibly have caught something
  first — F8 is the first such entry; accumulation is the signal to re-raise T7 with
  Eugene.
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

- **R-j — A microbenchmark's working set is part of its correctness** (new, T5, and it
  cost that session). T5's SAD microbench ran at a 1984-byte stride over 190 KB: every
  row access missed cache, the safe kernels' per-row overhead hid behind the misses,
  and it reported **0.82-1.33x** where the encoder reported **+16.8%**. The identical
  benchmark at a 64-byte stride over 4 KB — L1-resident, which is what a motion search
  is — reported 1.0-2.0x and agreed. **Size the working set from the real caller, state
  it next to the numbers, and measure both residencies when the caller's is unclear.**
  Four further microbench bugs T5 hit, each of which produced a plausible table first:
  no per-iteration `black_box` (LLVM caches results across a rotating input set);
  observing only one of several outputs (LLVM deletes the rest of the kernel while the
  opaque `extern "C"` raw call keeps computing it — a 4x handicap pointing the wrong
  way); a `const` stride where the real kernel takes a runtime `i32`; and variants timed
  in blocks rather than interleaved, so drift lands on whichever ran last. Also: two
  guesses were tested and refuted before anything was disassembled, exactly as in T4.
  **Disassemble first.** It found T5's mechanism in one pass.
- **R-k — Bisect a swap by file before optimising it.** T5 read as +13.9% overall; one
  extra build and two bench runs split it into a +16.8% half and a +0.57% half, which
  turned a wholesale revert into a narrow one and saved the intra work. The two-commit
  discipline makes the unswap cheap; a per-file bisect makes it *small*.
- **R-l — Calibrate with a null run before calling a bench reading noise (new, T8).**
  Run the **same binary in both slots** through the identical harness. Measured this
  session at median **+0.00%**, max +1.81%, zero rows over 5% — which turned a
  suspected artefact into a real +3.48%, i.e. the suspicion was wrong and only the null
  run could say so. It costs one extra pass of a bench that is already built, and it is
  now the standing step in front of any "that's just noise" conclusion. It also
  re-condemned **Spatial Ramps** (-38% between two runs of one binary): that row cannot
  detect anything, exclude it from every verdict.
- **R-m — After a swap, the "raw" side of a microbenchmark is the shim (new, T8, and
  it produced a whole clean-looking table of lies).** Calling `WelsFoo_c` post-swap
  measures the safe kernel wrapped in a shim against the safe kernel: T8's first
  six-kernel table read 0.99-1.07x across the board for exactly this reason, and the
  true figures were 0.69-1.47x. **Measure bodies *before* commit B, or recover them
  with `git show <commit-A>:<file>` into the bench crate** — which is what T4's log
  already said and what this session had to rediscover.
- **R-n — A per-call deficit scales with call count, so an encoder-side budget is not
  a decoder-side budget (new, T4's isolation).** The same `mc.rs` shims cost ~7-8% on
  the decode streams and **+7.6% median / +16.7% worst** on the encoder, because motion
  estimation calls MC far more times per frame than decoding does. Any family with
  encoder consumers must be measured on the encoder bench in its own right; a decode
  number is not evidence about it.
- **R-o — The exact-span trim is the per-row-bounds-check fix that works (new, T8).**
  Handed an open tail (`&plane[origin..]`) LLVM cannot relate `k * stride + W` to the
  slice length and re-checks every row; handed a window it knows is `(H-1)*stride + W`
  long, the checks fold. It took T8's walk from 1.82x to **1.02x**, and it is distinct
  from `PlaneCursor::row_windows`, which trims the span but then walks it with
  `chunks()` plus a per-row `[..W]`. Try it first on any kernel whose disassembly shows
  `slice_index_fail` sites in a row loop. **It does not rescue SAD** — measured, see
  D-perf-2's verdict — so it is a technique, not a cure.
- **R-p — The ratchet counts pointer types in prose (new, T8).** `raw_ptr` went +10 for
  nine casts; the tenth was `` `*mut i32` `` inside a `# Safety` doc comment. Writing
  those contracts is what this phase is *for*, so T6 and T7 will inflate `raw_ptr` just
  by documenting themselves. Judge the shape, not the sign — and expect the shape to
  include prose.

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

### T5-intra — DONE (`56a3dbf9`, `209d3c66`). T5-sad — REVERTED (`11f82d41`)
The two 16x16 luma predictors are behind shims at +0.57%, with per-kernel reference
shapes (V takes `&[u8; 16]` for the row above, H takes a cursor for the column left —
a shared parameter would have each contract claiming the other's reach) and a packed
`[u8; 256]` destination, which is what distinguishes them from the same-named 2-arg
decoder cousins converted in T3.

**T5-sad is PARKED and the box is closed.** D-perf-2 spent its one attempt this
session on the strongest available candidate — T8's exact-span trim (R-o), which had
just moved that family 2.51x → 1.02x on the same problem and was not in T5's rejected
set. On SAD, L1-resident, in the framing most favourable to it (plain slices, no cursor
construction), it measured **1.39-2.97x** across the seven shapes. Parked; T7's
SAD/SATD go prove-and-park; **a third investigation is out of scope for this phase no
matter how promising the next idea looks.**

The fourteen safe kernels stay in-tree, proven, uninstalled. The re-attempt point is
the **Phase 4 direct-dispatch checkpoint**, then caller conversion (ME, Phase 6.3). One
lead for whoever gets there, genuinely outside T5's rejected set: **`PlaneCursor::new`
is itself a large per-call cost at this block size** — paying the two constructions the
in-tree kernel pays moved 4x4 from 1.61x to 4.93x — so handing these kernels slices and
offsets instead of cursors is worth trying. That is a convention change, which is why
it was not tried here. And rebuild the harness first: the D-perf-2 one reads the
in-tree kernel at 2.9-8.7x where T5's read 1.55-1.68x, and moved raw `Sad8x4` 6.49 →
2.18 ns between two runs of the same binary (`perf_baseline.md` §Parked records both
the verdict and the caveat).

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

### T7 — the four encoder kernel files (plan for two sessions)
**69 fns by real count** — `encode_mb_aux.rs` 22, `encoder/decode_mb_aux.rs` 6,
`sample.rs` 8 (installers keep installing shims), `encoder/get_intra_predictor.rs` 33
— the biggest task in the phase, larger than T3 and T4 combined; the suggested split
is S3 = the first three files, S4 = `get_intra_predictor` + T9. This is where R-a's
mechanism gets its encoder-side test and where R-e is most likely to bite
(DCT/quant/SATD intermediates — check widths against both the old Rust and the C++
before writing each safe body). **`sample.rs`'s SAD/SATD are PROVE-AND-PARK, decided** — D-perf-2 spent its attempt
and the verdict is park (`perf_baseline.md` §Parked). Write the safe kernels, prove
them differentially, do **not** swap them, and do not spend a second measuring whether
this time is different. The rest of T7 converts normally; **microbenchmark first (R-h),
at the right residency (R-j), against bodies recovered per R-m**, and try R-o's
exact-span trim on anything whose disassembly shows `slice_index_fail` in a row loop. `c_vs_rust_bench` with
`FFMPEG` set is the end-to-end instrument and it works — T5 proved both that it catches
this class of regression and that it had been silently skipping, with sweep wall time as the coarse cross-check; if a regression shows up,
bisect by family — that's why the commits are two-per-family. SAD/SATD in `sample.rs`
have T4's call-density profile: expect scaffolding overhead, apply the two-ledger
split, and ledger any deficit with its evidence rather than burning the session
optimizing closed ground.

### T8 — DONE (`d41244c2`, `af98f6ab`)
Six kernels (not eleven), no dispatch table, no `no_mangle`, all six shims plain
`unsafe fn`. Bodies 0.69-1.04x except `VAACalcSadBgd_c` at **1.44x**, recorded with
its mechanism (per-quadrant accumulators cannot merge across a 16-wide row, so each
half fills half a vector register) rather than fixed. +3.5% median on the encoder.
Everything reusable is in R-l/R-m/R-n/R-o/R-p and `perf_baseline.md` §Phase 2 T8;
**F9** is in the findings doc.

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
signature surgery), no Phase 3 early start. **The D-perf-2 box is a non-goal
boundary, not a target:** when the half-session stop arrives, the state you are in is
the verdict — parking is success, not failure, and a third SAD investigation is out
of scope for the phase no matter how close the last experiment looked. Surplus time:
sharpen shim contracts and differential edge coverage — T6's contracts especially,
since they're the ones Phase 5 will lean on hardest.
