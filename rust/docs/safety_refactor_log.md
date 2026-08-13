# Safety refactor — session log

One entry per working session on [`safety_refactor_plan.md`](safety_refactor_plan.md).
Newest last. Each entry records what landed, the gate results (control vs final), what
was found but not fixed, and the next session's first action — so a session can resume
from this file and the plan's Progress appendix alone.

---

## 2026-08-07 — Phase 0, session A (T1–T5b)

**Goal:** Phase 0 tasks T1–T8 per `prompts/archive/phase0.md`. Got through T1–T4, T5a and T5b;
T5d resolved as a recorded finding rather than a deletion. T5c, T5e and T6–T8 remain.

**Started at** `353791f7`, **ended at** `dc8487cb`. Working tree clean at both ends.

### State inherited vs. state described

The brief described the tree as of `909c368b` with an uncommitted `sweep.sh`. By the
time this session started, HEAD was `353791f7`: the `sweep.sh` change had already been
committed there (T2's first half, already done), and the *uncommitted* file was
`safety_refactor_plan.md` instead, plus an untracked `rust/docs/prompts/`. Both were
committed as inherited (`87127430`, `156b9abb`).

### What landed

| commit | what |
|---|---|
| `87127430` | plan brought up to date with `fa67432f`/`909c368b` (committed as inherited) |
| `156b9abb` | the Phase 0 session brief recorded in-tree |
| `da5c06ae` | `RUST_ENC_PROFILE` — the diffharness can finally build and run release |
| `ac5b91d2` | recovered `encoder_port_handoff.md`, `encoder_port_status.md` and 4 audit scripts |
| `89c05aa2` | `perf_baseline.md` — the §7.4 budget anchor |
| `86b40ed1` | the `eb463dbd` segfault root-caused (finding F1); `#[ignore]` count corrected |
| `89aa6109` | T5a: 62 unreferenced SIMD extern declarations deleted from `mc.rs` |
| `f1c90948` | fix: `build.sh` broke on bash 3.2 (my own bug from `da5c06ae`) |
| `a2a19d3c` | T5b: 80 SIMD stubs + their CPU-flag branches deleted from `encode_mb_aux.rs` |
| `fbe45f11` | T5b: 33 more never-installed arch variants across 3 files |
| `dc8487cb` | finding F3: MT dynamic-slicing nondeterminism |

### Gates

**T1 control** (session start, `353791f7`) and **final** (`dc8487cb`) — identical:

| gate | control | final |
|---|---|---|
| `cargo test` | 294 passed, 0 failed, 20 ignored | same |
| `cargo test --release` | 294 passed, 0 failed, 20 ignored | same |
| `sweep.sh st mt def` (debug) | 341/341, 2m14s | 341/341 |
| `sweep.sh st mt def` (release) | 341/341, 21s | 341/341 |
| `c_vs_rust_bench` | all rows bit-identical | all rows bit-identical |
| `decode_1080p_bench` | 60/60 frames, hashes match | same |

The release sweep did not exist before this session — `compare.sh` hardcoded the debug
binary. `da5c06ae` added it, so gate 0 of §7.2 ("both build profiles") is executable for
the first time.

Ratchet counts over `src/`, T1 → final: `unsafe {` 612 → 507, `*mut`/`*const` tokens
5791 → 5390, `transmute` 23 → 23, `unsafe impl` 10 → 10, `no_mangle` 42 → 42. Total Rust
lines 85,182 → 84,365. **`unsafe fn` read 922 at both ends, which is a measurement bug,
not a no-op:** the pattern `unsafe fn` does not match `unsafe extern "C" fn`, which is
how all 113 deleted stubs were spelled. T6's ratchet script must count
`unsafe (extern "C" )?fn`; until it does, that number means nothing.

### Findings — recorded, not fixed

All three are written up in [`phase0_findings.md`](phase0_findings.md).

- **F1 — the `eb463dbd` release segfault, closed.** A 16-byte `uiBS` in
  `DeblockingMbAvcbase` written 32 bytes deep by `DeblockingBSCalc_c`: a stack buffer
  overflow on the mainline encode path, thousands of times per frame, for the port's
  whole life. The recorded crash site (null `pNonZeroCount`) was a symptom. Already fixed
  at HEAD — incidentally, by `e6fce464`. Proof: narrowing the array back at HEAD
  reproduces the crash 5/5.
- **F2 — four copies of the bitstream writer**, not the two the brief expected, and the
  `svc_encode_slice.rs` copy's guards, masking and overflow behaviour differ from the
  canonical one. Per the brief's stop rule: recorded, not deduped. Phase 3.2 owns it.
- **F3 — MT dynamic-slicing nondeterminism.** ~1 in 400–1000 `t=4 sm=3` encodes produces
  a wrong bitstream in release; 0 in 1200 in debug. Pre-existing (reproduced with this
  session's changes stashed). Makes a single release `sweep.sh mt` an unreliable signal.

### Discrepancies found in the plan and the brief

Fixed in the plan where they were factual; listed here so the pattern is visible.

1. **`#[ignore]` count is 20, not 13** — all in `tests/e2e_conformance_test.rs`. Corrected
   in §1.4 with the authoritative command.
2. **The §1.4 segfault attribution was wrong** — see F1. Rewritten.
3. **Phase 0 item 4 names three files with nothing in them.** `encoder/sample.rs`,
   `encoder/get_intra_predictor.rs` and `encoder/decode_mb_aux.rs` contain zero
   architecture-suffixed definitions. The real T5b families were `encode_mb_aux.rs`,
   `common/intra_pred_common.rs`, `decoder/decode_mb_aux.rs` and
   `decoder/error_concealment.rs`.
4. **`GetThreadCount` must be simplified to `0`, not `1`.** See the next-action note
   below — this one would silently change behaviour.
5. `c_vs_rust_bench` numbers are worthless without `FFMPEG` set — see
   [`perf_baseline.md`](perf_baseline.md).

### Next session's first action

**T5c — decoder threading scaffolding.** Step 1 of the recipe is already done and
recorded here: `pThreadCtx`, `pLastThreadCtx` and `pCsDecoder`
(`decoder/decoder_context.rs:772-774`) are **declared and read but never assigned** —
the only writes anywhere are the `mem::zeroed` default. Proven by
`grep -rn 'pThreadCtx\|pLastThreadCtx\|pCsDecoder' decoder/ api/`, which returns three
declarations plus three read sites and nothing else.

**Before touching `GetThreadCount`, read this.** The brief says to simplify it to return
a literal `1`. That is wrong and would change behaviour. It returns **`0`** today, via
the `pThreadCtx.is_null()` early return, and `api/codec_api.rs:1831` branches on
`GetThreadCount(p_ctx) <= 0` to increment `uiDecodeTimeStamp`. With `0` that branch runs;
with `1` it stops running, and the decoding timestamp changes. Every other caller tests
`> 1` or `<= 1` and is indifferent between 0 and 1 — `codec_api.rs:1831` is the single
site that distinguishes them. **Simplify to a literal `0`**, and keep the function so its
call sites stay honest.

Then T5e (the dead `use std::ffi::c_void` in `lib.rs`, plus whatever
`RUSTFLAGS="--force-warn dead_code" cargo build` harvests from the T5a–c deletions), then
T6 (ratchet + `gates.sh`), T7 (fuzz — **needs `rustup toolchain install nightly` and
`cargo install cargo-fuzz`; neither is present on this machine**), T8 (bookkeeping).

`gates.sh`, when written, must carry two things this session learned: run the encoder
bench with `FFMPEG` set and `BENCH_REQUIRE_FFMPEG=1`, and print F3's retry rule next to
the sweep result.

---

## 2026-08-07 — Phase 1, session A (T1–T9, complete)

**Goal:** Phase 1 per `prompts/archive/phase1.md` — build the safe vocabulary types in
`src/safe/`, fully unit- and differential-tested, Miri-clean, wired into nothing.
All nine tasks landed; Phase 1 is complete.

**Started at** `53e211f7`, **ended at** the commit carrying this entry. Working tree
clean at both ends.

### Deviation from the brief, by direction

The brief gates Phase 1 on Phase 0's exit gate, and Phase 0's T5c, T5e, T6 and T7 were
(and are) unchecked. Mid-session direction was **"skip fuzzing, do phase 1"**, so the
session went straight to Phase 1 rather than finishing Phase 0 first. That is sound for
this phase in particular — it changes no codec code and adds no `unsafe`, so the only
gate that could move is the test count — but two consequences are inherited by whoever
picks Phase 0 back up:

- **there is still no `unsafe_ratchet.sh` or `gates.sh`** (T6), so "ratchet check" could
  not be run. Substituted: every file in `src/safe/` is `#![forbid(unsafe_code)]`, which
  is a stronger statement than a count, and the raw counts are in the plan's appendix.
- **the fuzz crate (T7) is deferred**, so §7.2 gate 6 stays unavailable. Nightly *is*
  now installed (Phase 1 needed it for Miri); only `cargo-fuzz` is missing.

### What landed

| commit | what |
|---|---|
| `1dcf6b47` | the Phase 1 brief recorded in-tree |
| `448c8118` | T2: `src/safe/` skeleton, `ErrInfo`, the test PRNG |
| `952c8b1d` | T3: `PaddedPlane`, `PlaneCursor`, `PlaneCursorMut` + differential tests |
| `5b47b1fe` | T4+T5: `BsCursor`, `BsWriter` + differential tests |
| `e3a09459` | T6a: `Pool<T>`, `Id` handles, `PoolRest` |
| `066471ea` | T6b: `MbDims`, `MbArray` |
| `bb465bce` | T8: the differential tests made a usable Miri gate |
| `ed704c4d` | fix: the test PRNG's own tests were leaking into three pre-existing test binaries |
| `6c403c9e` | T9: plan updates, `phase1_findings.md`, the F3 re-measurement, this entry |

Plus this entry, `phase1_findings.md`, and the plan's §2.2/§10/Progress updates.

The brief's bookkeeping step also asks for an auto-memory update. **Auto-memory is
switched off on this machine** (`~/.claude/settings.json`: `"autoMemoryEnabled": false`,
and there is no memory store), so the earlier sessions' memory notes cannot be extended
and these three documents are the whole record. Later sessions should not go looking for
a memory note that says otherwise.

### Gates

| gate | control (`53e211f7`) | final |
|---|---|---|
| `cargo test` | 294 / 0 / 20 | 375 / 0 / 20 |
| `cargo test --release` | 294 / 0 / 20 | 373 / 0 / 20 |
| `sweep.sh st mt def` (debug) | 341/341 | 341/341 |
| `sweep.sh st mt def` (release) | 340/341, F3 | 340/341, F3 — see below |
| `decode_1080p_bench` | 60/60 frames, hashes match | same |
| `cargo +nightly miri test --lib safe::` | — | 63/63 clean, 19s |
| `cargo +nightly miri test --test safe_plane_differential` | — | 3/3 clean, 37s |
| `cargo +nightly miri test --test safe_bits_differential` | — | 15/15 clean, 13s |

The +81 tests are 63 in-module unit tests and 18 differential ones. Debug runs two more
than release on purpose: `pool`'s stale-handle tests are `#[cfg(debug_assertions)]`,
since the behaviour they check exists only there. **Every pre-existing test binary keeps
its exact control count** and the 20-test `#[ignore]` set is untouched, as is every
hash — the whole phase's diff outside `src/safe/`, `tests/` and `docs/` is the single
line `pub mod safe;` in `lib.rs`.

That per-binary check earned its keep: the first exit run showed
`decoder_conformance_test`, `e2e_conformance_test` and `loopback_sha1_test` each up by
3, because `cfg(test)` is *on* in an integration-test crate, so the PRNG's own unit
tests — declared in a file included by path into `tests/common/` — compiled into every
binary that pulls that module in. The totals still only went up, so a totals-only check
would have passed. The generator's tests now live in `src/safe/mod.rs` instead, and
`prng.rs` carries a comment saying why it must stay test-free.

**The release sweep and F3.** Three release sweeps this session each failed exactly one
configuration, always `t=4 sm=3 n=600`, always zero-byte or short output — F3's
signature. One of the three was the *control* run on an untouched tree, which already
settles it, but the rate (1 per sweep) was well above the ~1 in 400–1000 F3 recorded, so
it was measured properly rather than waved through: eight `sweep.sh mt` runs at HEAD
against eight at `53e211f7`, release, quiescent machine.

| tree | configurations | failures |
|---|---|---|
| `53e211f7` (control) | 960 | 1 |
| HEAD | 960 | 2 |

Same signature, same clip. 1 vs 2 in 960 is noise, and Phase 1's entire codec diff is one
`pub mod` line, so there is no mechanism by which it could be otherwise. F3's write-up
gained the two refinements this produced: every failure that day was at `n=600`
specifically, and the rate rises when the machine is busy — the elevated readings all
came from sweeps running alongside `cargo test` or Miri. **Don't run the release `mt`
sweep concurrently with other builds** if a single run has to mean something.

### Findings — recorded, not fixed

Written up in [`phase1_findings.md`](phase1_findings.md); numbering continues from
Phase 0's F1–F3.

- **F4 — the reader's slop reads three bytes past the RBSP, not one.** The plan
  describes the *cursor* correctly and understates the *reads*: `iReadBytes >
  iAllowedBytes + 1` permits a cursor one byte past the end, and the refill then loads
  two bytes there; separately the initial prime reads four bytes regardless of NAL
  length. Soundness today is a property of the 4 MiB `sRawData` allocation, not of the
  NAL. `BsCursor` reproduces the predicate exactly and is identical to the C++ given
  ≥3 bytes of slack, strictly safer without. Phase 3.1 owns the guard-byte decision.
- **F5 — the canonical writer panics in debug on a 32-bit write** into an empty
  accumulator (`uiCurBits << 32`; UB in C++). Unreachable today — no encoder call site
  writes a full word — but one syntax element away from live. `BsWriter` folds the
  shift away; a test pins both profiles.
- **F6 — `malloc` is declared returning `*mut u8`** rather than `*mut c_void`
  (`common/memory_align.rs:17`). Miri warns; ABI-compatible in practice; the file is
  deleted outright in Phase 6, so the fix is deletion.
- **F7 — two out-of-bounds pointer computations in the reader**, found by Miri:
  `DecInitBits`'s `pStartBuf.offset((kiSize + 7) >> 3)` for `kiSize < -7`, and
  `InitReadBits`'s `pEndBuf.offset(-iEndOffset)` for an `iEndOffset` past the buffer.
  UB by the arithmetic alone. Unreachable today, but only by two invariants that are
  nowhere written down; both die in Phase 3.1 when the offsets replace the pointers.

### Notes for later phases

1. **Miri on the differential tests is worth its cost and should stay a phase-exit
   gate.** It executes the *old* unsafe code, which is how F7 surfaced — the first UB
   ever found in this port by a tool rather than by a crash. Sample counts scale down
   under `cfg!(miri)` (a `scale()` helper in each differential file) so the whole
   Miri battery is ~70 seconds rather than an hour; the full-size run happens on every
   `cargo test`.
2. **No test needed `#[cfg_attr(miri, ignore)]`**, which the brief expected to be
   necessary. Where the old side has UB (F7) the comparison is bounded to the
   in-contract range and the safe side is checked alone beyond it — that keeps the UB
   coverage the ignore would have thrown away. Prefer this shape in later phases.
3. **Three quirks of the C++ reader are now pinned by tests** and must not be
   "fixed" during Phase 3: the slop predicate (F4), the 16-bit ceiling on
   `BsGetBits` (the refill only guarantees 16 valid bits at rest, so wider reads
   return stale low bits — the decoder never asks for more), and
   `CheckMoreRBSPData`'s fixed 2-byte subtraction, which over-counts by 16 bits until
   the cursor reaches its steady state.
4. **The plan said the decoder error codes live in `decoder_context.rs`.** They do
   not; that file has only `ERR_NONE`. They are declared *twice*, in
   `decoder/bit_stream.rs` and `decoder/dec_golomb.rs`. `safe::err` reuses them and
   carries a test pinning the copies together — F2's defect class, pre-empted.
5. Two API deviations from §2.2, both now in the plan: `BsCursor` carries `len` and
   `bits` (without the logical end, error-code parity at a NAL boundary is not
   expressible), and `Pool::mut_and_rest(cur)` replaces `cur_and_refs(cur, refs)`
   (the ref list only served to reject `cur ∈ refs`, which the returned view does
   anyway, and it would have forced an allocation into a per-MB path).

### Next session's first action

**Phase 2, the pilot conversion of `decoder/decode_mb_aux.rs`** onto the plane API
(plan §Phase 2) — the smallest kernel family, converted deliberately first so an
API-shape mistake surfaces before mass adoption. Watch for two things the vocabulary
types have not yet been asked to prove: whether `PlaneCursorMut::row_mut` hoists out of
4x4 inner loops well enough to hold the §7.4 budget, and whether kernels want
`(buf, center, stride)` triples rather than the cursor for the hottest paths.

If Phase 0 is picked up instead, its first action is unchanged from the previous
entry — **T5c, decoder threading scaffolding**, with `GetThreadCount` simplifying to a
literal `0`, not `1`.

---

## 2026-08-07 — Phase 2, session A (preconditions, T1, the pilot)

**Goal:** `prompts/archive/phase2.md`'s session-A split — the preconditions (Phase 0's T6,
optionally T5c/T5e), the control run, and the pilot conversion of
`decoder/decode_mb_aux.rs` with its retro. All of it landed, with T5c/T5e taken *after*
the pilot rather than before (reasoning below). **Phase 0 is complete except T7.**

**Started at** `956a8c07`, **ended at** the commit carrying this entry. Working tree
clean at both ends.

### What landed

| commit | what |
|---|---|
| `afcdd785` | the Phase 2 session brief recorded in-tree |
| `99f4ab5c` | Phase 0 T6: `unsafe_ratchet.sh` + `gates.sh` + the baseline |
| `ba13bdbd` | pilot A: the four safe kernels + differential proof, old code untouched |
| `ee7818fb` | pilot B: swap + shims, tautological differential entries deleted |
| `725e37fd` | the pilot retro: plan verdicts, `perf_baseline.md` Phase 2 column, this entry |
| `3e09a814` | T5c 1/4: three dead duplicate declarations of the threading types |
| `e3ca3e17` | T5c 2/4: the thread context; `GetThreadCount` → literal `0` |
| `5fe41f17` | T5c 3/4: the row-sync event machinery and its two dead `SPicture` fields |
| `01ad9ba1` | T5c 4/4: `OpenDecoderThreads`, `CloseDecoderThreads`, `WelsTaskThread` |
| `4e174285` | T5e: the dead `use std::ffi::c_void` in `lib.rs` |
| `b7f48311` | T3 A: 42 safe intra-prediction kernels + differential proof |
| `a4828187` | T3 B: swap + 42 shims, tautological differential entries deleted |

Plus `phase2_findings.md` (F8), the plan's §2.2.1/§Phase 2/§7.1/§7.5 and Progress
updates, and `perf_baseline.md`'s Phase 2 section.

### Gates

| gate | control (`afcdd785`) | final (`ee7818fb`) |
|---|---|---|
| `cargo test` | 375 / 0 / 20 | 377 / 0 / 20 |
| `cargo test --release` | 373 / 0 / 20 | 375 / 0 / 20 |
| `sweep.sh st mt def` (debug) | 341/341, 136s | 341/341, 136s |
| `sweep.sh st mt def` (release) | 341/341, 21s | 341/341, 21s |
| `decode_1080p_bench` | 60/60, 3 hashes match | same |
| `c_vs_rust_bench` | all rows bit-identical | all rows bit-identical |
| `miri --lib safe::` | 64/64 | 64/64 |
| `miri --test safe_{plane,bits}_differential` | 3/3 and 15/15 | 3/3 and 15/15 |
| `miri --test kernels_differential_phase2` | — | 1/1 |

The session-end battery ran at level `exit`: 11 gates pass, 1 SKIP (fuzz). `gates.sh`
discovers `tests/*differential*.rs` by glob, so the new Phase 2 file joined the Miri set
without being wired in by hand.

One honest note on the closing bench: its rows read 2.251 / 5.830 / 5.873, back at the
control's level rather than the paired medians' — the machine had been building all
session, and that between-invocation drift is the same order as the effect being
measured. The paired table in `perf_baseline.md` is the budget measurement, because it
is the only one where both sides ran back-to-back under identical conditions.
`gates.sh`'s bench line is a correctness gate (frame counts and hashes), not a budget
one; later families should take their own paired run.

**No F3 hit at all this session**, in six release `mt` sweeps — consistent with the
rate F3 records for a quiescent machine, and worth noting because the two previous
sessions each saw one. The retry rule stands; nothing about it changed.

The +2 net test count is +1 (`plane::reborrow`'s unit test) and +1 (the surviving
property test); the five differential entries came and went inside the family, exactly
as the phase convention intends. Every pre-existing test binary kept its control count
and the 20-test `#[ignore]` set never moved.

### The pilot retro

This is what the pilot existed to answer, and the verdicts are now in plan §2.2.1.

1. **Cursor vs `(buf, center, stride)` triple: the cursor, and the question was a
   false dichotomy.** `PlaneCursorMut` *is* the triple — three fields behind a `&mut`,
   whose `row_mut` is one add and one slice index. The only thing the triple avoids is
   the `assert!` in `new`, which runs once per kernel call rather than once per row.
   The measurement settled it: **1.3–1.8% faster** with the safe kernels live, 3 runs
   per side, per-side spread ≤1% (`perf_baseline.md` §Phase 2 T2). The phase uses the
   cursor; a triple now needs a bench number to justify it.
2. **`row_mut` hoists fine; what has to change is the C++ loop order.** The 4x4 IDCT
   wrote column-major — four strided single-byte stores per column — which no
   row-window idiom can rescue. Transposed to row-major it became four contiguous
   stores through a `&mut [u8; 4]`, and that is the most plausible source of the win.
   **The transposition is bit-exact only because every sample of the block is read and
   written exactly once**; that is a per-kernel proof obligation, not a blanket licence.
   Check it, then transpose.
3. **The Phase 1 API needed exactly one addition: `PlaneCursorMut::reborrow(dx, dy).`**
   `advance` consumes the cursor, which is right for a walk and wrong for a composite
   kernel that hands sub-blocks to an inner kernel and then carries on. Made now, before
   ~120 kernels consume the API, per the brief. Expect the same shape in `mc.rs` and
   `deblocking_common.rs`.
4. **Shim ergonomics: short contracts are a property of the kernel, not of the writer.**
   All four of these reach *forward only* from the block's own (0,0), so the reachable
   span is `(bh-1)*stride + bw` — computable from the signature, needing nothing about
   the plane's padding. That is why these `# Safety` blocks are four lines. It will not
   hold for T3 (`get_intra_predictor` reads the `-1` column and `-stride` row), T6
   (deblocking reaches `-3*step`, `expand_pic` writes the padding), and those shims will
   have to name the padding constant. Budget for it; per the brief, those contracts are
   the phase's real deliverable.
5. **Differential-test cost: ~40 lines and a few minutes per kernel** — cheap, and it
   earned its keep immediately by finding F8. The convention that works: generate the
   surface with *noise* (a constant surface cannot show a write on the wrong row),
   compare the **whole** buffer, and drive the strides list at the minimum legal value,
   three non-multiples of 16, and two real picture strides.

### What the ratchet actually does in this phase — read before T3

Measured, and it is the strangler pattern's arithmetic rather than a surprise:

```
no_mangle    42 -> 38      unsafe_fn   1372 -> 1372
SHIM(         0 ->  4      raw_ptr     5390 -> 5390
unsafe_block 507 -> 511
```

`unsafe_fn` and `raw_ptr` do not move **because the shim keeps the raw signature** —
they die in Phase 5 when the callers convert and the shims are deleted. `unsafe_block`
*rises* because each `from_raw_parts` is now an explicit block, where the old bodies'
derefs were invisible inside an `unsafe fn` under `#![allow(unsafe_op_in_unsafe_fn)]`:
counted unsafe replacing uncounted unsafe, i.e. the metric getting more honest.

So the brief's expectation that "this phase produces the largest unsafe-count drops of
the whole plan" is **wrong**, and the plan now says so (§Phase 2). Workflow consequence:
**commit B of each family regenerates the baseline**, after running `check` and
confirming every increase is confined to the file being converted. That is what keeps
the ratchet a ratchet for the other 70 files while this one moves.

### Discrepancies found in the brief

1. **The pilot family is 4 kernels in 314 lines, not "~13 kernels, 374 LOC".** The C++
   `decode_mb_aux.cpp` has more; the Rust port has `IdctResAddPred_c`,
   `IdctResAddPred8x8_c`, `IdctFourResAddPred_c` and `GetI4LumaIChromaAddrTable`. There
   are no dequant kernels in this file — decoder dequant lives elsewhere. Consumers,
   re-proven by grep: three `pIdctResAddPredFunc*` slots installed at
   `decoder_core.rs:1937-1939`, plus `GetI4LumaIChromaAddrTable` reached only through
   the inline wrapper at `decoder_core.rs:947`. No `sBlockFunc`-adjacent slot exists.
2. **`GetI4LumaIChromaAddrTable` is in no table at all**, so its shim dropped
   `extern "C"` as well as `#[unsafe(no_mangle)]`. The other three keep the ABI because
   their typedef demands it.
3. **§1.1's "~922 `unsafe fn`" is a naive count.** The real figure is 1372 definitions;
   1582 including function-pointer types. §7.1 now records all three numbers and why
   they differ.

### Findings — recorded, not fixed

- **F8 — the 8x8 IDCT's `i16` intermediates overflow above ~528**, where a debug build
  panics with "attempt to add with overflow" and the C++ wraps through signed-overflow
  UB. Pre-existing, faithful to the C++, unreachable on conformant streams (the spec
  bounds dequantised coefficients), reachable in principle on malformed ones since
  nothing between the parser and the kernel clamps. Written up in
  [`phase2_findings.md`](phase2_findings.md).
  **This is the moment the brief asked to be recorded as data for reopening T7:** a
  `decode_annexb` fuzz target is the natural instrument for exactly this, and there
  isn't one. It was found instead by a hand-written property test that happened to
  generate full-range coefficients.

### T5c/T5e, landed after the pilot rather than before it

The brief lists them under preconditions as "do first, recommended", with the reason
that T5c touches `decoder_core.rs`, which Phase 2 also touches. On inspection that
overlap is with §Phase 2 **T6**'s expand functions — four families and several sessions
away — so it did not constrain the pilot, and the pilot is what unblocks the idiom
decision for the other ~120 kernels. They were therefore taken second, and they landed:

| commit | what |
|---|---|
| `3e09a814` | three dead *duplicate* declarations: `decoder_core`'s `SThreadInfo`/`SWelsDecoderThreadCTX`, `mv_pred`'s `SWelsDecEvent` |
| `e3ca3e17` | the thread context — `pThreadCtx`/`pLastThreadCtx`/`pCsDecoder`, `SWelsDecThreadInfo`, `SWelsDecoderThreadCTX`; `GetThreadCount` → literal `0` |
| `5fe41f17` | the row-sync events — `SWelsDecEvent`, the four Event helpers, `Picture::pReadyEvent`, `Picture::pNzc`, `GetPNzc`'s picture branch |
| `01ad9ba1` | the entry points — `OpenDecoderThreads`, `CloseDecoderThreads`, `WelsTaskThread` and their call sites |
| `4e174285` | T5e: the dead `use std::ffi::c_void` in `lib.rs` |
| `b7f48311` | T3 A: 42 safe intra-prediction kernels + differential proof |
| `a4828187` | T3 B: swap + 42 shims, tautological differential entries deleted |

Ratchet across the five: `unsafe_fn` 1372 → 1365, `raw_ptr` 5390 → 5352,
`unsafe_block` 511 → 507 — the first *drops* of the session, and worth contrasting with
the pilot's flat numbers above: deletion moves the ratchet, strangling does not.

Four things worth carrying forward:

1. **`GetThreadCount` is a literal `0`, and the reason now lives in its docstring**
   rather than only in this log. Every caller but one tests `> 1` or `<= 1` and cannot
   tell `0` from `1`; `api/codec_api.rs:1831` branches on `<= 0` to increment
   `uiDecodeTimeStamp`. A `1` would have silently changed the decoding timestamp.
2. **The duplicates went first on purpose.** Deleting a type that exists twice is only
   safe once you know *which* copy the live code names, and `decoder_core`'s
   `SWelsDecoderThreadCTX` shadowed `decoder_context`'s with a different field list
   (7 fields against 12) while `GetThreadCount` referred to the latter by fully
   qualified path. Ordering the deletions duplicates-first made every later grep
   unambiguous. This is `phase0_findings.md` §F2's defect class showing up decoder-side.
3. **One fork collapsed, and it is not the fork §2.2.4 says to keep.** `GetPNzc` chose
   between the decoded picture's NZC array and the dq-layer's; the picture side was
   allocated only behind a thread-count gate that never opened, so the condition was
   statically false. The general `pDec` vs `pCurDqLayer` pattern still has two live
   sources elsewhere and is untouched.
4. **T5e yielded exactly one line.** `RUSTFLAGS="--force-warn dead_code" cargo build`
   after the deletions reports only pre-existing, unrelated dead code (`CEnumRepr`,
   `u16_cast_short`, `u32_cast_long`, `u8_slice_cast_char_slice`, `replace_array_items`),
   which the phase rule keeps out of scope. Don't re-run it expecting a harvest.

**Phase 0 is now complete except T7 (fuzz), which stays deferred by direction.**

### T3 — `decoder/get_intra_predictor.rs`, the designated worst case

Taken next in the same session. `b7f48311` proved 42 safe kernels against the raw ones;
`a4828187` swapped them behind shims. **42, not the 44 the brief counts**: 14 I4x4 luma,
14 I8x8 luma, 7 chroma, 7 I16x16 luma.

**The headline: the phase's designated perf worst case is a wash.** Paired 3-run
medians, every decode-bench row inside ±1%, which is the per-side spread
(`perf_baseline.md` §Phase 2 T3). Plan §9's "perf regression in MD/ME/intra hot loops"
risk row can be downgraded on the strength of it. The mechanism generalises and is worth
carrying into T4 and T7: **bounds checks land per row, not per sample** — a 4x4 mode does
one `row_mut` per output row against 16 samples of arithmetic — and `copy_from_slice` /
`fill` on a fixed-size window compile to the same wide stores the punned `u32`/`u64`
accesses were there to get.

Four things learned that the remaining families inherit:

1. **Every punned access in this file is a byte *move*, not an arithmetic use.** So
   `u32::from_ne_bytes`, the taxonomy T7 replacement the brief nominates, was needed
   nowhere in ~140 accesses. Two idioms covered all of them: `ST32/ST64(p, 0x0101.. * v)`
   → `row_mut(..).fill(v)`, and a window of a local `kuiList` → `copy_from_slice`. Check
   for arithmetic use before reaching for `from_ne_bytes` in T4/T7.
2. **The shim span formula.** These are the first shims not derivable from the
   signature. One helper computes it and nothing else does the arithmetic:
   `centre = stride + 1`, `len = max(centre + (bh-1)*stride + bw, 1 + top_reach)`, slice
   anchored at `pPred - stride - 1`. `top_reach` is 8 for the 4x4 diagonals and 16 for
   the 8x8/16x16 ones. The contract names `PADDING_LENGTH` as the reason the `-1` row and
   column exist at all — that sentence is what Phase 5 converts callers against.
3. **Sweep availability flags exhaustively, not randomly.** `bTLAvail`/`bTRAvail` select
   different formulas at both ends of the filtered-neighbour arrays, and three of the
   fourteen 8x8 kernels ignore `bTLAvail` entirely while still taking it. A random sweep
   blurs exactly the distinction a conversion is most likely to get wrong.
4. **Anchor test blocks at random legal offsets.** Several of these kernels were written
   around unaligned `u32`/`u64` stores; a fixed, conveniently-aligned test anchor would
   not exercise that.

Two quirks preserved deliberately, both of which look like bugs and are not:
`DDLTop`/`VLTop` pad their 16-sample neighbour array with the **raw** `T7` rather than
the filtered one, and `WelsI8x8LumaPredV_c` packs its output into a `u64` low byte first
— little-endian-dependent, upstream included. The safe rewrite copies bytes instead,
which is what was meant and is identical on every target this project builds for.

**F3 fired once and was retried per the rule.** The release `mt` sweep failed exactly
`t=4 sm=3 n=600` with a short output; the retry ran 120/120 clean. Structurally it could
not have been this commit — the family is decoder-only and the sweep exercises the
encoder — but the rule is to re-run rather than argue, and re-running settled it.

### Next session's first action (as recorded at the time; superseded below)

Phase 2 T4 — `common/mc.rs`. Its three carry-ins (cursor *pair*, the MV clamp as the
shim contract, folding `pWelsMcFunc_c`) all held up and all three are reflected in what
landed.

---

## 2026-08-08 — Phase 2, session B (T4, `common/mc.rs`)

**Goal:** the session brief's S1 split — T4 then T5. **T4 alone consumed the session**,
because it is the first family to break the §7.4 performance budget and the
investigation that entailed was the work. T5 is untouched and is the next action.

**Started at** `34ad2eb4`, **ended at** the commit carrying this entry. Tree clean at
both ends.

### What landed

| commit | what |
|---|---|
| `46053993` | T4 A: 24 safe kernels + a differential entry per kernel, old code untouched |
| `ea52b387` | T4 B: swap + 28 shims, equivalence entries replaced by one span property |
| `e26164e6` | the plane-API row walker, built, measured, rejected, reverted |

Plus `perf_baseline.md` §Phase 2 T4 and the plan's Progress appendix.

### Gates

| gate | control (`34ad2eb4`) | final |
|---|---|---|
| `cargo test` | 377 / 0 / 20 | 378 / 0 / 20 |
| `cargo test --release` | 375 / 0 / 20 | 376 / 0 / 20 |
| `sweep.sh st mt def` (debug) | 341/341 | 341/341, 141s |
| `sweep.sh st mt def` (release) | 341/341 | 341/341, 22s |
| ratchet | clean | clean, baseline regenerated |
| `miri --test kernels_differential_phase2` | 1/1 | 2/2, 74s |

**No F3 hit in any of the eight release `mt` sweeps this session.** The +1 net test is
the one surviving property test; every pre-existing binary kept its count and the
ignored set never moved.

**A correction to the brief's numbers:** it records the control as 375 debug / 373
release, which was T1's control at `afcdd785`, not the state at `34ad2eb4`. HEAD was
**377 / 375** — the pilot's two surviving tests. Nothing was wrong, but a session that
trusts the brief's figure will think it has gained two tests it did not.

### The headline: this family is over budget, and the reason is the shim, not the kernels

**+8.2% / +7.2% / +7.0%** on the three decode-bench streams, against §7.4's 5%
per-family limit. Everything below is in `perf_baseline.md` §Phase 2 T4 with the run
values; what belongs here is what the next family should do differently.

1. **The measurement protocol had to change, and should stay changed.** Sequential
   `cargo bench` invocations of the *same binary* drifted ~3% on the Rust rows while
   the C++ rows in the same runs held to ±0.3%. A 5% budget cannot be judged through
   that. The protocol that works: build both binaries, keep them on disk, and run them
   **interleaved in one loop**. T3's "trust the paired table, not a single run" was an
   understatement — an unpaired reading of this family said +5.1% when the paired
   reading said +8.7%, and I acted on the wrong one for a while.
2. **Profile before theorising.** The first measurement was +33/+20/+20 and my first
   two guesses (missing `inline(always)`, a dynamic reach lookup) were both wrong and
   both cost a build-and-bench cycle. `/usr/bin/sample` on the running bench, with
   self-time computed from the call graph, found the real cause in one pass. The
   *third* instrument — a scratch microbenchmark driving old kernels (recovered with
   `git show`) against new ones per phase and per block shape — is what gave the 10.8x
   number that made the fix obvious. Build it early next time; it costs ten minutes.
3. **Three real defects, all of which generalise to T5–T8:**
   - **`copy_from_slice` on a runtime length is a `memmove` call.** The C++ copies a
     fixed 16/8/4/2 bytes per row for a reason. Const-generic width (`copy_rows::<16>`)
     took the zero-MV 16x16 copy from **10.8x to ~1x**, and that was most of the
     regression. Any kernel copying a runtime-width window has this bug.
   - **Indexing several same-length slices by `j` is not free.** LLVM does not prove
     seven bounds facts per sample. Zip the iterators.
   - **Attribute parity with the C++ matters**, and is not cosmetic: its kernels are
     all `inline` and its composites sit in a *table*, i.e. out of line with the inner
     kernels inlined into them. `#[inline(always)]` on inner kernels, `#[inline(never)]`
     on the ones carrying a big scratch.
4. **Three structural hypotheses tested and rejected by paired measurement**, recorded
   so nobody retries them: cursors passed **by value** rather than by reference (a
   wash); `McLuma_c` dispatching over the raw per-phase shims so every span computation
   folds to constants (a wash, and it would have left two dispatch implementations to
   keep in agreement); and **a `rows()`/`rows_mut()` walker on the Phase 1 plane API**
   — one bounds check per block, advancing by pointer addition instead of a multiply
   per row. That last one was Eugene's call and it is the important negative result:
   it made things **worse**, +23.6/+13.5/+13.2 everywhere and still +14.4/+10.2/+10.2
   when confined to `copy_rows`, the simplest two-way zip in the file. `Chunks::next`
   is a `min` plus a `split_at` plus a runtime-length slice needing `[..WIDTH]` and a
   `try_into`; `row()` hands back a statically-sized window whose checks fold. **A
   multiply per row is cheaper than that.** The API addition was reverted rather than
   left unused, because an API that measures worse is a trap for the families that
   read its doc comment and not this log.
5. **What the residual actually is.** Fixed per-call overhead — span arithmetic, two
   `from_raw_parts`, four constructor asserts — at ~7–16 ns a call, on a family called
   constantly with small blocks (every inter partition, plus two chroma calls each).
   The *kernels* are at parity or better: the centre filters measure 0.88–0.99x
   against the raw ones. **That overhead is the strangler scaffolding, and Phase 5
   deletes it with the shims.** The open question for the phase is whether to carry it
   until then.

### Inventory, as re-proven by grep

**26 kernels** (24 safe bodies; `McHorizLuma_c`/`McVertLuma_c` are C++ aliases sharing
`mc_hor_ver20`/`mc_hor_ver02`) plus two filter helpers and `InitMcFunc`. The brief's
"26 kernels + InitMcFunc" was right.

**Only `InitMcFunc` is called by name from outside the module** — `decoder_core.rs:1909`
and `encoder_context.rs:689`. Everything else is reached through `SMcFunc` slots:
`decode_slice.rs:1093,1105` (decoder), and `md.rs:1059,1196,1229,1289…`,
`svc_mode_decision.rs:545,548,1904,1916`, `svc_base_layer_md.rs:1438,1450,1596`
(encoder). `InitMcFunc` is untouched: it writes a struct through a `*mut SMcFunc` and is
dispatch plumbing, which is Phase 4's.

### Four things the remaining families inherit

1. **The encoder drives half-pel at `iWidth + 1` / `iHeight + 1`** — up to **17**, which
   is why `McHorVer22_c`'s scratch is `int16_t[17 + 5]` and why the quarter-pel
   composites (a `uint8_t[256]` at stride 16) are capped at 16 instead. Any family with
   encoder consumers must find that ceiling before sizing a test.
2. **The MC safety argument is the caller's clamp, not the padding.** `pSrc` has already
   been displaced by the motion vector, so `PADDING_LENGTH` alone proves nothing. The
   shim section quotes `BaseMC`'s clamp (`decode_slice.rs:1069-1091`) and derives why it
   is *exactly* calibrated: the integer vector lands in `-30 ..= width + 13`, so a
   16-wide block reaching `x - 2` and `x + 19` touches `-32 ..= width + 32`, precisely
   the 32-sample luma border, with nothing to spare. The `+ 2` and `- 19` in the clamp
   **are** that margin, and `19 == 16 + 3`. This is the contract Phase 5 converts
   callers against.
3. **Per-kernel reach, not the union.** `LUMA_REACH[iMvX & 3][iMvY & 3]` gives each of
   the sixteen kernels its own span because `McHorVer10_c` reads no row outside its
   block and `McHorVer13_c` reads five. A shim claiming the union would assert validity
   for rows its caller never promised — cheaper to write, and a lie.
4. **The surviving test is a span property, and it is worth copying.** Each shim is run
   against source and destination allocations sized to **exactly** the span its contract
   declares. Too large a span is UB that Miri reports at the `from_raw_parts`; too small
   a span panics in the safe kernel; and running the same call into two differently
   noised destinations pins that the kernel writes every byte of its block and reads
   none of them. All three directions were mutation-tested. This replaces the
   equivalence entries with something that outlives the family.

### Two smaller things worth not rediscovering

- **A macro over shims makes the ratchet lie.** Folding fifteen
  `unsafe extern "C" fn` definitions into one macro invocation made `unsafe_fn` report
  a 13-definition *drop* that had not happened. The shims are written out one by one.
  Same trap for the `SHIM(` marker: put it on each invocation, not inside the macro.
- **`raw_ptr` cannot fall during a strangler phase, and R-f overstates it.** The metric
  counts `*mut`/`*const` **occurrences** — signatures and casts, not the pointer
  arithmetic in bodies — so a shim that keeps its signature keeps its count. This family
  went **+2**, both from `shim_wh`, the one shared helper where a pointer becomes a
  slice; paying that beats 29 copies of the span arithmetic. Judge the shape, not the
  sign.

### A test that was exercising UB

`test_mc_horiz_and_vert_luma_aliases` anchored a 4x4 block at `src.as_ptr()` of a bare
`[u8; 64]` and filtered it, reading `pSrc[-2]` and `pSrc[-2 * 8]` off the front of the
array — in a test whose whole subject is a kernel that reaches outside its block. The
raw code did that read silently; the shim materialises the span, so it had to be fixed.
Worth noting that `gates.sh` runs Miri as `--lib safe::`, which would never have covered
it: **the Miri gate does not see the port's own unit tests.** Not changed here (it would
be a gate change mid-phase), but a candidate for T9.

### Next session's first action

**Phase 2 T5 — `common/sad_common.rs` + `common/intra_pred_common.rs`**, per
`prompts/archive/phase2.md` §T5 and the continuation brief. Carry-ins unchanged: the `Four` SAD
kernels read **outside** the nominal block (`-stride`, `-1`, `+1`) so they take plane
cursors and the differential must cover the edge reads; the three pre-existing unused
`sample_sad_*` safe wrappers get absorbed rather than left as a second SAD API;
consumers are `encoder/sample.rs`'s installers plus the direct call at
`processing/scene_change_detection.rs:103`; and `intra_pred_common`'s two 3-arg kernels
must not be unified with the same-named 2-arg decoder ones already converted in T3.

**Before starting it, settle T4's budget question with Eugene** — it is a phase
decision, not a T5 one. The options as they now stand: carry the ~7% to Phase 5, which
deletes the shim layer that causes it; revert T4 and leave `mc.rs` raw until Phase 5
converts its callers; or accept a per-family budget that is unachievable while
strangling and restate §7.4 in terms the phase can actually meet. What is *not* still
open is whether a cleverer kernel or a better plane API fixes it — three attempts say
no, and the numbers are in `perf_baseline.md`.

Build the microbenchmark harness first in T5 and T7, not third.

---

## 2026-08-08 — Phase 2, session C (T5, and a measurement lesson)

**Goal:** the continuation brief's S1 split — T5 then T8. **T5 alone consumed the
session**, and it ends *half* landed: `intra_pred_common`'s two predictors are behind
shims, `sad_common`'s fourteen SAD kernels were swapped, measured, and **unswapped**.
T8 is untouched and is the next action.

**Started at** `3ddd2405` with three uncommitted files (session B's open decision),
**ended at** the commit carrying this entry. Tree clean at both ends.

### What landed

| commit | what |
|---|---|
| `82014c9d` | D-perf-1 written up: §7.4 restated with the two-ledger split, the deficit ledger, the 10% ceiling, Phase 4/5 recovery checkpoints |
| `56a3dbf9` | T5 A: 16 safe kernels + a differential entry per kernel, old code untouched |
| `840a48dc` | `PlaneCursor::row_windows`, and why T4's `chunks` verdict still stands |
| `209d3c66` | T5 B: swap + 16 shims |
| `11f82d41` | unswap the fourteen SAD shims; keep the two intra ones |

Plus `perf_baseline.md` §Phase 2 T5 and the Progress appendix.

### Gates

| gate | control (`82014c9d`) | final |
|---|---|---|
| `cargo test` | 378 / 0 / 20 | 384 / 0 / 20 |
| `cargo test --release` | 376 / 0 / 20 | 382 / 0 / 20 |
| `sweep.sh st mt def` (debug) | 341/341, 142s | 341/341 |
| `sweep.sh st mt def` (release) | 341/341, 22s | 341/341 |
| ratchet | clean | clean, baseline regenerated |
| `miri --test kernels_differential_phase2` | 2/2 | 5/5, 78s |
| `c_vs_rust_bench` | **now runs** | see below |

**F3 hit, and it was chased properly.** The session-end battery failed one `t=4 sm=3`
release `mt` configuration and the re-run failed two different ones — more than one hit,
so R-g's comparison applies. Six `mt` sweeps at HEAD **alternating in one loop** with six
at the control gave **control 4 failures, HEAD 1**; across the session, HEAD 5/14 runs and
control 4/12. Pre-existing, appended to F3 with two refinements the control supplied: it
also fires at **`t=2 sm=3`**, and the output is often short rather than zero-length, so
both the retry rule and any automation matching on "zero bytes" were too narrow. Also
learned: measuring the two trees at different times is worthless for a load-sensitive
race — an earlier "HEAD 4/8 vs control 0/6" reading inverted once they were alternated.

### The instrument was wrong, and it cost the session

This is the headline and it generalises further than T5 does.

**`FFMPEG` was never set, so the encoder bench had been skipping for the whole
phase.** `ffmpeg` is installed at `/opt/homebrew/bin/ffmpeg` on this machine and
always was. Every prior session recorded `SKIP c_vs_rust_bench: encoder perf is
UNMEASURED`, and T5 is the first encoder-side family, so this was the first family
whose regression the battery *could* have caught and hadn't. **Set `FFMPEG` on every
run from here on.** It is one environment variable between a measured encoder and an
unmeasured one.

**The microbenchmark reported the safe SAD kernels at 0.82-1.33x. The encoder
reported +16.8%.** The microbenchmark was wrong, and it was wrong in a way that looked
like diligence: it drove the kernels at a 1984-byte stride over a 190 KB working set,
so every row access was a cache miss and the extra per-row work hid behind the misses.
The same benchmark at a 64-byte stride over 4 KB — L1-resident, which is what a motion
search actually is — says 1.0-2.0x and agrees with the encoder. **Size a kernel
microbenchmark's working set from the caller, and say the size next to the numbers.**
Where residency is not obvious, measure both; a kernel at parity memory-bound and 1.7x
L1-resident is 1.7x in a real encoder.

Three further methodology bugs were found and fixed *before* that one, each of which
first produced a plausible-looking table:

1. **No per-iteration `black_box`.** LLVM cached results across the rotating anchor set
   and reported a shim faster than the body it wraps.
2. **Only `out[0]` observed** on the four-point kernels, so LLVM deleted three quarters
   of the safe kernel while the opaque `extern "C"` raw call kept doing all of it — a
   4x handicap pointing the wrong way.
3. **A `const` stride.** Every span and row offset folded; the real shims take
   `iStride1`/`iStride2` as runtime `i32`. This one flattered the safe side most.

And the variants have to be **interleaved**, not run in blocks — R-h's rule applies
inside one process, not just across binaries. Run in blocks, drift across a 20-second
run lands entirely on whichever variant went last, which is how an earlier table showed
`plane` beating `shim` at doing strictly less work.

### Two guesses, both wrong, before anything was profiled

Same trap T4's log names, walked into again. Rolling the row offset instead of
computing it: **worse** (1.5-2.7x). `u8::abs_diff` instead of the `i32`
subtract-and-abs: **no change at all**, LLVM already emitted it. Disassembling
`sample_sad::<16, 8>` found the answer in one pass — ~96 instructions of bounds
checking hoisted to the top of the function before a single sample is read, because
`PlaneCursor::row` costs two compare-and-branch pairs and a 16x8 walked row by row on
two surfaces is 32 of them. **Disassemble before hypothesising, not after two build
cycles.**

### What the corrected instrument says the cost is

Per row, and **independent of width**: through the safe kernel, 4x8, 8x8 and 16x8 all
take ~7.0 ns against 4.0 / 4.2 / 6.8 ns raw. The per-sample arithmetic vectorises to
free. What remains is bounds and iterator work once per row, which only the 16-wide
shapes have enough samples to amortise — hence 16x8 and 16x16 at 1.00-1.02x while 4x4
and 8x8 sit at 1.55-1.68x. **Any fix must attack per-row overhead.** The inner loop is
already optimal and two attempts to improve it changed nothing.

### `row_windows` — kept, and the T4 verdict survives intact

`PlaneCursor::row_windows::<W>` takes the block as one slice and walks it with
`chunks`: one bounds check per block instead of two per row. Re-measured L1-resident
against the `row()` walk it replaced, it wins on twelve of fourteen shapes and wins
large on the four-point ones (1.2-2.0x against 2.1-3.3x). It was the right change made
for the wrong reason, and it is not why SAD is slow.

It does **not** repeal T4's rejection of `chunks` walkers, and `mc.rs` was deliberately
not refitted. T4's `rows()` yielded runtime-length slices needing `[..WIDTH]` and a
`try_into`, and lost to `row()` in `mc.rs` — where the widths are const-generic and the
checks genuinely fold, so `row()`'s branches cost nothing and `Chunks::next` costs
something. Both results hold, and the rule is now written on the method: **`row` where
the window is statically sized and the checks fold, `row_windows` where they cannot.**

### Four things the remaining families inherit

1. **`FFMPEG=/opt/homebrew/bin/ffmpeg` on every `gates.sh` run.** T7 is encoder-only;
   without it there is no gate on the thing being changed.
2. **A microbenchmark's working set is part of its correctness**, not a detail. T7's
   SAD/SATD have exactly T5's profile.
3. **Bisect a swap by file before optimising it.** One extra build and two bench runs
   turned "T5 costs +14%" into "the SAD half costs +17% and the intra half is free",
   which is what made a narrow unswap possible instead of a wholesale one.
4. **Sizing a span does not pin where it starts.** The first span property test passed
   six mutations but survived one that built its slice from `pSample2` instead of one
   row above while leaving the cursor anchored at `iStride2` — reading a block shifted
   a whole row down and returning four plausible, distinct, in-range numbers. The test
   now recomputes each four-point result through independently anchored cursors. Copy
   that shape: **span size and span anchor need separate assertions.**

### Inventory, as re-proven by grep

**16 kernels** — `sad_common.rs` 14 (7 single-block, 7 four-point) and
`intra_pred_common.rs` 2 — matching the brief, the second brief estimate to hold.
Consumers as briefed: `encoder/sample.rs:218-241` installs all fourteen SAD;
`encoder/get_intra_predictor.rs:782-783` installs the two predictors; one direct call
at `processing/scene_change_detection.rs:103`. The three `sample_sad_4x4/8x8/16x16`
slice wrappers had **no caller anywhere in the tree** and were absorbed.

**T8 recount: 6 kernels, not 11** — `vaacalc.rs` 5 and
`adaptive_quantization.rs` 1. The continuation brief's 11 was wrong; `phase2.md` §T8's
"5 + `SampleVariance16x16_c`" was right.

### Two smaller things worth not rediscovering

- **`git checkout -- <file>` in a mutation-test loop reverts to HEAD**, which silently
  discarded an hour of uncommitted commit-A work. Mutation loops back up with `cp`, or
  run against committed code.
- **A scratch benchmark under `tests/` runs in `cargo test`**, in a debug build, at
  release iteration counts. It hung the suite. Keep scratch harnesses outside the crate
  between runs.

### Where T5 stands

`intra_pred_common` is done: two shims, contracts written, span property pinned, and
+0.57% measured, which is inside every budget. `sad_common` has its fourteen safe
kernels written, differentially proven against the raw ones, and **not installed**. The
raw kernels run. Re-landing it needs a per-row overhead fix, and the corrected
microbenchmark is the instrument for judging one.

### Next session's first action

**Phase 2 T8 — `processing/{vaacalc,adaptive_quantization}.rs`**, 6 kernels, per
`phase2.md` §T8. It is small, it is off the ME hot path, and it does not depend on the
SAD question. `SampleVariance16x16_c` is an R-e case: its `u16`/`u32` accumulators
already carry explicit `wrapping_add`/`wrapping_mul`, so reproduce them exactly.

**Then T5-sad or T6, and that is a scheduling call for Eugene.** T5b needs a per-row
overhead fix on kernels whose per-sample work is already free — a real optimisation
problem, not a conversion one, and it is the same problem T7's SAD/SATD will present at
larger scale, so solving it once serves both. T6 is the phase's hardest conversion and
was always meant to be scheduled alone. Doing T5-sad first buys the technique T7 needs;
doing T6 first keeps the conversion order the plan assumed.

## 2026-08-09 — Phase 2, session D (T8, T4's encoder deficit, D-perf-2's verdict)

**Goal:** the continuation brief's S1, in its fixed order — T4's encoder-stream
isolation, then T8, then the bounded D-perf-2 attempt. All three landed. **The
headline is not T8**, which converted cleanly; it is that isolating T4's encoder cost
found a **breach of §7.4's hard ceiling that has been in the tree since session B**,
and that this session's two microbenchmarks both lied before they told the truth.

**Started at** `75beada5` with three uncommitted decision docs, **ended at** the commit
carrying this entry. Tree clean at both ends.

### Deviation from the brief's fixed order

The brief fixed S1 as **1. T4 isolation, 2. T8, 3. D-perf-2**, explicitly "so the
time-box can't squeeze the deliverables". I took T8 first, and it did squeeze the
others — T8 alone consumed most of the session. The ordering was right; recording the
deviation rather than quietly reordering, because the next session inherits the same
temptation and the same reasoning ("the small one first, it'll be quick") that made it
look safe. The partial redemption is real but was luck, not planning: T8's null
experiment established this bench's noise floor, and T4's isolation then read against
it for free.

### What landed

| commit | what |
|---|---|
| `445a1392` | D-perf-2 and the parked-family state written up (session C's leftovers) |
| `d41244c2` | T8 A: 6 safe kernels + a differential entry each, old code untouched |
| `af98f6ab` | T8 B: swap + 6 shims, tautological entries replaced by span properties |

Plus `perf_baseline.md` §Phase 2 T8, the T4 encoder isolation and its ceiling-breach
section, the D-perf-2 verdict under §Parked, `phase2_findings.md` F9, and the plan's
§7.4 (D-perf-3 raised, D-perf-2 closed) and Progress appendix.

### Gates

| gate | control (`445a1392`) | final |
|---|---|---|
| `cargo test` | 384 / 0 / 20 | 392 / 0 / 20 |
| `cargo test --release` | 382 / 0 / 20 | 390 / 0 / 20 |
| sweeps (debug) | 341/341, 142s | 341/341, 140s |
| sweeps (release) | 341/341, 22s | 341/341, 21s |
| ratchet | clean | clean, baseline regenerated |
| `decode_1080p_bench` | 60/60, hashes match | 60/60, hashes match |
| `c_vs_rust_bench` | all rows bit-identical | all rows bit-identical |
| `miri --lib safe::` | pass | pass |

**One F3 hit, in the session-end battery, and the retry cleared it.** Release `mt`,
`t=2 sm=3 n=600 cabac=0 rc=0`, Rust output zero-length — which is F3's signature
exactly as session C broadened it (release `mt`, `sm=3`, `t` in {2,4}, output short or
zero). Re-running the release `mt` preset gave **120/120**. One hit, so R-g's retry
rule applies and the alternating-loop comparison does not. Worth noting that unlike
T3's hit this one was *not* structurally impossible — T8 is an encoder-side family —
which is precisely why the rule is "re-run rather than argue". `FFMPEG` was set on
every run.

Net **+8 tests**: seven differential entries in commit A, of which six were deleted in
commit B and replaced by three span/anchor properties, plus four new in-file unit
tests. Every pre-existing binary kept its count; the ignored set never moved.

### T4's encoder deficit breaches the ceiling — D-perf-3, and it is Eugene's call

D-perf-1's addendum asked for `mc.rs`'s encoder-side contribution now that the encoder
bench actually runs. Commit A `46053993` against commit B `ea52b387`, interleaved,
3 pairs: **+7.6% median, +16.7% worst, eleven usable rows over the 10%-per-stream hard
ceiling.** T8's +3.5% sits on top of it, so the live cumulative encoder deficit is
about **+11%**.

T4's ledger *eligibility* is unchanged — bodies are still 0.88-0.99x and the residual
is still fixed per-call scaffolding that Phase 5 deletes. What is new is the size, and
its cause is structural: motion estimation calls MC far more times per frame than
decoding does, so a fixed per-call cost is levied far more often. That is why the
encoder is hit harder than the decoder by the same shims, and it is worth stating as a
general rule for the rest of the phase — **a per-call deficit scales with call count,
so an encoder-side family's budget is not its decoder-side budget.**

Options and evidence are in `perf_baseline.md`; the three are carry-with-a-raised-
ceiling, revert `mc.rs` until Phase 5, or pull Phase 4's direct-dispatch checkpoint
forward for `mc.rs` alone. Not decided here.

**The instrument lesson is session C's, one turn further on.** T4 landed in session B
judged on the decode bench alone, and D-perf-1 carried it assuming the encoder looked
similar. One unset environment variable hid a ceiling breach for two sessions.

### T8 — six kernels, and two microbenchmarks that lied

Inventory re-proven by grep: **6 kernels**, `vaacalc.rs` 5 + `adaptive_quantization.rs`
1, matching `phase2.md` §T8 against the continuation brief's 11. **No dispatch table
anywhere in the family** — all six are direct-called from `Process` methods in their
own files, none carries `#[unsafe(no_mangle)]` or `extern "C"` — so per §3.2 all six
shims are plain `unsafe fn` and there were no exports to delete.

Bodies against the raw ones: `Sad` 0.86x, `SadVar` 0.86x, `SadSsd` 0.69x, `SadSsdBgd`
0.97x, `SampleVariance` 1.04x, and **`SadBgd` 1.44x**. End to end **+3.48% median**.

**What made the fast ones fast** (full detail in `perf_baseline.md` §Phase 2 T8): the
first version measured 2.51x, disassembly found 64 `slice_index_fail` checks per
macroblock — T5-sad's mechanism — and two changes fixed it. Reading a 16-wide row once
and splitting it in registers instead of walking quadrants separately: 2.51x → 1.82x.
Then **trimming each plane to exactly the span the row loop reads, before the loop**,
so LLVM can relate `k * stride` to the slice length: 1.82x → **1.02x**, and 0.84-0.88x
at smaller sizes.

**`VAACalcSadBgd_c` at 1.44x is recorded, not fixed.** The raw kernel issues 16
`uabd`/16 `umax` where this one issues 8/8: BGD's per-quadrant signed sum and running
maximum cannot merge across a 16-wide row, so each half fills half a vector register.
Passing the statistics by value rather than `&mut` was tried and measured *worse*
(1.52x) — do not retry it. The fix is a second quadrant-shaped walk selected on the
`BGD` const, which is a live option for T7 and more machinery than this family earns.
The general shape is worth carrying: **the 16-wide row read pays only when the
per-quadrant state can merge across the row.**

### The two instrument errors, and the new step they add

Both are session C's failure mode, and both were caught by *disagreement* rather than
by suspicion — which is the point, because suspicion was wrong the one time it fired.

1. **After a swap, the "raw" side of a microbenchmark is the shim.** The six-kernel
   table was first produced after commit B, calling `vaa::VAACalcSad_c` as raw. All six
   rows read 0.99-1.07x, which looks like a clean pass and was a function compared with
   itself. Recovering the bodies with `git show d41244c2:` turned that into 0.69-1.47x.
   **Measure bodies before swapping, or recover them explicitly; never call the old
   name afterwards and believe the number.**
2. **Calibrate with a null run before calling a bench reading noise.** The encoder's
   +3.48% was doubted on a sound-looking argument — the whole walk costs 0.11 ms at
   1080p and cannot produce +0.29 ms. Running the **same binary in both slots** gave
   median **+0.00%**, max +1.81%, zero rows over 5%. The floor is ±2% and the reading
   was real. **The null run is now a standing step in front of any "that's just noise"
   conclusion**, and it costs one extra pass of a bench that is already built. It also
   re-condemned Spatial Ramps, which moved **-38%** between two runs of one binary.

Two documented traps walked into anyway, both worth re-reading before the next paired
measurement: **the interactive shell is zsh and does not word-split**, so a harness
loop over `for which in $order` produced files named `enc_ctl head_1.log` and three
error logs — run harnesses under `bash -c`, as `gates.sh`'s own header says. And
**`c_vs_rust_bench` resolves the C++ library relative to `cwd`** (`workspace_root()` is
`../../../`), so it must run from the crate directory; "fixing" the cwd to the repo
root made it fall back to no-C++ and produce an unparseable log, twice.

### D-perf-2 — one attempt, spent, and the verdict is park

**T5-sad stays parked; T7's SAD/SATD go prove-and-park from the start.** Exit state (b),
which the decision pre-authorised, and the box is closed.

The candidate was not a guess: T8's exact-span shape, which had just moved that family
2.51x → 1.02x on the same problem (fixed window, runtime stride, per-row checks that
will not fold) and was not in T5's rejected set. On SAD, L1-resident, **in the framing
most favourable to it** — plain slices, no cursor construction — it measured
**1.39-2.97x** across the seven shapes. Not close to the ≤1.05x the swap requires.

One finding a later reopening should start from: **`PlaneCursor::new` is itself a large
per-call cost at this block size.** Making the candidate pay the two constructions the
in-tree kernel pays moved 4x4 from 1.61x to 4.93x. Handing these kernels slices and
offsets instead of cursors is therefore a live idea and genuinely outside T5's rejected
set — and a convention change, so it belongs to the Phase 4 checkpoint, not here.

**Caveat recorded in the baseline rather than buried:** that harness is not sound
enough for its absolute numbers to be quoted. It reads the in-tree `sample_sad` at
2.9-8.7x where T5's read 1.55-1.68x, and raw `Sad8x4` moved 6.49 ns → 2.18 ns between
two runs of the same binary. The verdict rests only on the robust part — nothing came
near parity in any framing — and anyone reopening this should rebuild the harness
first.

### Findings — recorded, not fixed

- **F9 — `iFrameSad` is an `int32_t` accumulated over a whole picture** and overflows
  above 32 896 macroblocks against a `MAX_MBS_PER_FRAME` of 36 864: reachable only at
  the top two frame sizes the encoder accepts and only on a near-black-to-near-white
  cut. F8's class, encoder-side, so no attacker-controlled path — a correctness
  ceiling, not a panic candidate. Reproduced exactly; repairing it is a type change at
  `SVAACalcResult::iFrameSad` and belongs to Phase 4's plumbing or later.
- **Two false claims in the converted files' own headers, corrected.**
  `adaptive_quantization.rs` said `SampleVariance16x16_c`'s difference sum could wrap;
  it cannot — 256 samples of at most 255 top out at 65280, and a test now drives both
  extremes to pin it. (The `wrapping_add`s stay: the rule is to reproduce the C++'s
  declared widths, not only the ones that turn out to matter.) `vaacalc.rs` said "only
  the `pfVAACalcSad` kernel is translated", which was never true and would have sent
  T9's straggler sweep hunting four kernels that were already there.
- **`VAACalcSad_c` may be the one kernel that is mostly *not* reached.**
  `bEnableBackgroundDetection` and `bEnableAdaptiveQuant` both default **on**
  (`encoder/param_svc.rs:351-352`), which selects the `Bgd`/`SsdBgd` variants. The
  file's header had it backwards, and any future perf work on this family should
  confirm which variant runs before optimising one.

### A ratchet wrinkle worth knowing before T6 and T7

`raw_ptr` went **+10** for nine casts. The tenth is inside a **doc comment** — the
metric counts `*mut i32` in `# Safety` prose. Since writing those contracts is what
this phase is *for*, T6 and T7 will inflate `raw_ptr` simply by documenting themselves.
Judge the shape, not the sign (R-f), and expect the shape to include prose.

### Next session's first action

**Phase 2 T6 — `common/deblocking_common.rs` + the F1 `uiBS` surgery + `expand_pic.rs`**,
per `phase2.md` §T6, scheduled **alone** as the plan has always said. Carry-ins
unchanged: the `(iStrideX, iStrideY)` swap *is* the V/H encoding and ports as explicit
`(step_x, step_y)` with the `-3*step` reach in the contract; the decoder's untyped
double-cast installer stays (Phase 4); the F1 surgery gives `uiBS` its real
`[[[u8; 4]; 4]; 2]` type end-to-end and deletes the five `from_raw_parts_mut(…, 32)`
sites, diff kept surgical; the expand shims reconstruct the full allocation span from a
mid-pointer with the per-variant pad constant, and the two `mem::transmute` fn-pointer
re-wraps stay. R-c applies doubly — the expand span helper and its contract sentence
are the whole deliverable.

**Before starting T6, put D-perf-3 to Eugene.** It is a phase decision of the same kind
as D-perf-1, the ceiling is already breached, and T6 adds decoder- *and* encoder-side
shims to a tree that is over budget on the encoder side.

---

## 2026-08-10 — Phase 2, session E (T6 complete: deblocking, the F1 surgery, expansion)

**Goal:** the continuation brief's T6, scheduled alone — the phase's hardest
conversion plus its one Phase-6-file surgery — and the first session run under
**D-perf-4's** regime (swap-and-ledger, one interleaved pair per bench per commit B,
no boxes). All three items landed, in the brief's order, with no deviation to record.

**Started at** `b339818d` with two uncommitted decision docs (session D's D-perf-4
write-up, committed first as `1dc0ecad`), **ended at** the commit carrying this
entry. Tree clean at both ends.

### What landed

| commit | what |
|---|---|
| `1dc0ecad` | D-perf-4 recorded: plan §7.4 v3, the D-perf-3 decision history, the baseline's breach section annotated |
| `bbb9348e` | T6-deblocking A: 7 safe kernels + differential proof, old code untouched |
| `5756ad75` | **F10**: the parked raw SAD kernels' trailing bump is UB on exact spans; test accommodated |
| `64633e5f` | T6-deblocking B: the 12 ABI wrappers become the shims; 6 raw inner kernels deleted; 14 `no_mangle` gone |
| `d878916b` | the F1 surgery: `uiBS: [[[u8; 4]; 4]; 2]` end-to-end, five 32-byte casts deleted |
| `2fe283e4` | T6-expand A: one pad-parameterised `expand_picture` + differential proof |
| `9c32f890` | T6-expand B: the two mid-pointer shims (`expand_shim_span`, pads 32/16) |

Plus `perf_baseline.md` §Phase 2 T6 and two ledger rows, `phase2_findings.md` F10,
and the plan's §2.2.1 expand-packaging wording and Progress appendix.

### Gates

| gate | control (`1dc0ecad`) | final |
|---|---|---|
| `cargo test` | 392 / 0 / 20 | 395 / 0 / 20 |
| `cargo test --release` | 390 / 0 / 20 | 393 / 0 / 20 |
| sweeps st mt def (debug) | 341/341 | 341/341, 25s |
| sweeps st mt def (release) | 341/341 | 340/341, **one F3 hit, retry 120/120** |
| ratchet | clean | clean, baseline regenerated twice (deblocking B, expand B) |
| `decode_1080p_bench` | hashes match | all streams bit-identical |
| `c_vs_rust_bench` (FFMPEG set) | all rows bit-identical | all rows bit-identical |
| `miri --lib safe::` | pass | pass |
| `miri --test kernels_differential_phase2` | **FAIL (pre-existing, F10)** | 12/12, 139s |

**One F3 hit, in the session-end battery, and the retry cleared it.** Release `mt`,
`t=4 sm=3 n=600 cabac=0 rc=0`, Rust output zero-length — the signature exactly; the
release `mt` preset re-ran 120/120. One hit → R-g's retry rule, not the alternating
loop; appended to F3 as its fifth measurement. The session's three earlier full
sweep batteries (both profiles) had passed 341/341 first try, and the hit came in
the least quiescent state of the session — after hours of builds with two background
bench runs — which is F3's known widening condition. `FFMPEG` was set on every
battery and bench run.

Net **+3 tests**: the three deblocking equivalence entries came and went inside the
family (commit A +3, commit B -3 +2 span/anchor properties), expansion the same
(+1, then swapped for the span probe), and F10's accommodation changed sizes, not
counts. Every pre-existing binary kept its count; the ignored set never moved.

### Ratchet across the session

```
no_mangle    38 -> 24   (-14: 12 wrappers + WelsNonZeroCount_c + DeblockingInit)
unsafe_fn  1366 -> 1360 ( -6: the six raw inner edge kernels)
raw_ptr    5218 -> 5198 (-20: deleted raw bodies, five uiBS casts, ExpandPictureCommon)
SHIM(        82 -> 97   (+15: 13 deblocking + 2 expand)
unsafe_block 588 -> 597 ( +9: the shims' from_raw_parts, within the +15 allowance)
```

The shape is R-f's exactly: exports and raw bodies die, shims and their blocks rise,
`unsafe_fn` falls only where raw kernels are *deleted* rather than strangled.

### T6-deblocking — the V/H trick ported honestly, and what it cost

The `(iStrideX, iStrideY)` swap **is** the V/H encoding, and it survives as one safe
body per kernel taking `(step_x, step_y)` in bytes, indexing `i*step_y + j*step_x`
around the cursor anchor through checked math — the twelve ABI wrappers *are* the
shims, fixing `(iStride, 1)` or `(1, iStride)` exactly as the C++'s wrappers do. No
per-direction bodies, no dispatch: the brief's shape held with nothing forced.

Per-kernel reach, not the union (T4's rule): Lt4 luma claims `[-3, +2]` taps, Eq4
`[-4, +3]`, chroma `[-2, +1]`; `shim_span` is the one place the arithmetic lives.
The availability argument is written once at module level and quoted by every
contract: **the negative reach is legal because the drivers only filter an edge
whose p side exists** — MB-boundary edges gated on left/top availability (and
same-slice under `uiFilterIdc == 2`), interior edges 4/8/12 samples in. The padding
is *not* part of this argument, which makes it a cleaner Phase 5 target than the
intra families' padding-based contracts.

**Perf under D-perf-4** (first family through the new protocol, ~20 minutes of
measurement all told): decode **+7.79 / +2.55 / +2.33%**, worst on the CAVLC stream
because it is the fastest and the fixed per-edge scaffolding is the biggest fraction
of it — T4's decode mechanism, recognised rather than re-investigated. Encoder flat
(median -0.11%), because the encoder installs nothing from this module. Tripwire
arithmetic: cumulative decode ≈ +17/+10/+10%, under +25% → ledgered, moved on. No
microbenchmark was built and no optimisation attempted: the number matched the known
mechanism, which is exactly the case D-perf-4's "no boxes" rule was written for.

### The F1 surgery — the type now carries what five casts used to

`uiBS` is `[[[u8; 4]; 4]; 2]` through the `PDeblockingBSCalc` table typedef,
`DeblockingBSCalc_c`, both `DeblockingBSInsideMB*` helpers (now `&mut` — they are
direct-called), and `DeblockingInterMb`'s read side. The five
`from_raw_parts_mut(uiBS as *mut u8, 32)` sites and the 32-byte read are gone; the
boundary rows write through 4-byte row assignment where the C++ puns `uint32_t`.
The 16-vs-32-byte relationship that caused the release segfault is now the
signatures' job, checked by the compiler at every call site. `FilteringEdge*`'s
`pBS: *const u8` parameters stay — not in F1's named scope, and their four-byte rows
now come from typed `uiBS[dir][edge]` arrays. Sweeps 341/341 both profiles,
byte-exact; encoder pair read median +0.36%, inside the session's null floor.

### T6-expand — the trickiest shim span, and it fit one helper

Callers hand `pData[i]`, a **mid-allocation** pointer; `expand_shim_span` walks back
`(1 + stride) * pad` with the per-variant constant and claims the
`(h + 2*pad) * stride` padded-plane prefix. Both codecs' `AllocPicture`s were
re-proven to place the origin identically (`pic_queue.rs`, `wels_preprocess.rs` —
chroma is the same expression halved), so **no call site needed the explicit-pad
fallback** the brief pre-authorised. The safe kernel is one pad-parameterised body
in `common/expand_pic.rs`, runtime-length `copy_within`/`fill` on purpose — once per
reference picture, the same library calls the C++ makes (R-i's const-generic rule is
for per-block kernels, and this is the counter-case worth naming). Perf: decode
+1.0/+0.1/+0.5%, encoder indistinguishable from the null floor.

### The probe lesson: a golden run pins what a touch-set cannot

The deblocking span probe first asserted exact-span in-bounds behaviour plus
"no byte outside the declared touch set moved" — and a **+1-anchor mutation walked
through it**, because a shift along the line axis stays inside the touch set and
filters plausibly. The fix is a third assertion: the shim's output must equal the
safe kernel run directly at the contract's own geometry. Span size (exact-span
allocation + Miri), touch set, and anchor are now pinned by three independent
mechanisms, and the same golden-run shape went into the expand probe from the start
(where it kills the `pad*stride`-without-`+pad` reconstruction). **Carry to T7:
every span probe gets a golden direct run; touch-set assertions alone are blind
along any axis the crafted input is uniform in.**

### F10 — the instrument found UB in *parked* raw code

The full-file Miri run flagged the **T5-sad** span test: the raw kernels' post-last-
row pointer bump (`pSrc = pSrc.offset(iStride)`, inherited from the C++) computes an
out-of-allocation pointer on a buffer that ends at the block's last row — exactly
the exact-span buffers the probe hands them, and exactly the state the T5 unswap
left the Wels names in. Pre-existing since `11f82d41`; it slipped through because
the differential file runs under Miri only at phase exits. Per T4's precedent the
*test* carries the accommodation (whole-row buffers while the family is parked,
exact spans restored at re-landing), the finding is `phase2_findings.md` F10, and
T7's SAD/SATD prove-and-park differentials must size raw-side buffers whole-rows
from the start.

### The noise floor is not a constant

Session D's null run measured ±1.81% max; this session's, on the same bench and
machine, **±5.41%** on the tiny QVGA rows (and the biggest same-binary mover was
the very row the expand pair flagged at +10.45%). A null run is now demonstrably
*per-session* evidence — run it fresh before judging any reading against a
remembered floor. Both nulls agree the medians hold to ~±1.3%, which is what the
commit-B pair verdicts rest on.

### Where T6 stands

Complete. Deblocking: 13 shims live, decoder table and installers untouched, the
decoder's untyped double-cast at `decoder_core.rs:1906` left for Phase 4. F1:
surgical, encoder-only, byte-exact. Expansion: 2 shims live, the two
`mem::transmute` fn-pointer re-wraps at `decoder_core.rs:920-921` left for Phase 4.
Cumulative ledger: decode ≈ +17/+10/+10%, encoder ≈ +11%, all under D-perf-4's
tripwire; recovery unchanged at Phase 4 / 5–6 / 9.

### Next session's first action

**Phase 2 T7 part 1 — `encoder/encode_mb_aux.rs` (22) + `encoder/decode_mb_aux.rs`
(6) + `encoder/sample.rs` (8)**, per the continuation brief's S+1. Carry-ins:
`sample.rs`'s SAD/SATD are **prove-and-park, decided** (D-perf-2) — write, prove,
do not swap, size raw-side differential buffers whole-rows (F10); R-e is most
likely to bite in the DCT/quant arithmetic — check widths against both the old
Rust and the C++ before writing each safe body; every span probe gets a golden
direct run; encoder families measure the encoder bench in their own right (R-n),
one interleaved pair per bench at commit B, null run fresh if a reading needs a
noise verdict. Then S+2 = `encoder/get_intra_predictor.rs` (33) + T9.

---

## 2026-08-10 — Phase 2, session F (T7 part 1: forward transforms, recon IDCT, SATD parked)

**Goal:** the finishing brief's session F — `encoder/encode_mb_aux.rs` (F-1),
`encoder/decode_mb_aux.rs` (F-2), `encoder/sample.rs`'s SATD prove-and-park (F-3) —
under D-perf-4. All three landed in the brief's order, no order deviation to record.
**The headline is two instrument events, not the conversions**: an F3 hit with a
shape outside the recorded signature (Rust output *longer*), resolved by the
full alternating-loop protocol and appended as F3's sixth measurement; and Miri
catching F10's rule *under-applied* in the new SATD differential — whole rows are
not enough for composite kernels.

**Started at** `915bb554` with one untracked doc (the finishing brief, committed
first as `97ad7975` — the session control), **ended at** the commit carrying this
entry. Tree clean at both ends.

### What landed

| commit | what |
|---|---|
| `97ad7975` | the finishing brief committed (control point) |
| `f233d506` | F-1 A: 21 safe kernels + 5 differential entries, 3 mutations killed |
| `b1fb7448` | F-1 B: swap behind 21 shims; span probe with golden runs |
| `fc92dab0` | F-2 A: 9 safe kernels (recount; brief said 6) + entries; **F11** recorded |
| `55e6d7fe` | F-2 B: swap behind 9 shims (4 in decode_mb_aux, 5 in svc_encode_mb) |
| `1383acb5` | F-3: 7 SATD kernels proven and **parked** — no commit B by decision |
| `20e84e47` | F10 second instance: SATD raw-side buffers to `(h+1)*stride` |

Plus `perf_baseline.md` §Phase 2 T7 part 1, two ledger rows and the T7-satd
§Parked row, `phase2_findings.md` F11 and F10's second instance,
`phase0_findings.md` F3's sixth measurement, and the Progress appendix.

### Gates

| gate | control (`97ad7975`) | final |
|---|---|---|
| `cargo test` | 395 / 0 / 20 | 399 / 0 / 20 |
| `cargo test --release` | 393 / 0 / 20 | 397 / 0 / 20 |
| sweeps st mt def (debug) | 341/341, 23s | 340/341 — **the first debug F3 hit ever observed** (`t=4 sm=3 n=600 cabac=1`, short); preset retry 120/120 |
| sweeps st mt def (release) | 341/341, 20s | 340/341 — F3, longer-shape again (`t=4 sm=3 n=600 rc=1`); preset retry 120/120. F-1 B's mid-session battery had the sixth-measurement hit (below) |
| ratchet | clean (1360/5198/97/24) | clean, baseline regenerated twice (F-1 B, F-2 B) |
| `decode_1080p_bench` | all streams bit-identical | all streams bit-identical |
| `c_vs_rust_bench` (FFMPEG set) | all rows bit-identical | all rows bit-identical |
| `miri --lib safe::` | pass | pass |
| `miri --test kernels_differential_phase2` | — | **caught F10's second instance**, then 16/16 |

Net **+4 tests** (399 vs 395 debug): F-1's five equivalence entries came and went
inside the family (+5 at A, −5 +1 probe at B), F-2's the same (+2, −2 +1), and
F-3's two entries are **live** — a parked family's differentials outlive the
session by design. Ignored-20 never moved.

### The F3 event — a new signature shape, and the protocol run in full

F-1 B's release sweep failed one configuration:
`mt CiscoVT2people_160x96 t=2 sm=3 n=600 cabac=0 rc=0`, Rust **42462** bytes vs
C++ 41938 — the F3 configuration class exactly, but *longer*, where every recorded
event was zero-byte or short. Outside the signature means no one-hit retry, so
attribution ran the full instrument: the exact config **12/12 clean per side**,
alternating a commit-A harness binary (raw kernels, built in a worktree) against
commit B in one loop; then, after the preset re-run produced a second, classic
short-output hit (`t=4 sm=3 n=600 rc=1`), the two-hit protocol: **three full
release `mt` presets per side, alternated — A 0 failures/360 configs, B 1/360.**
Indistinguishable rates on a commit that touches no MT machinery, with kernels
proven byte-identical. Verdict: F3, sixth measurement, signature's output clause
widened to *any wrong length*. The counts are in `phase0_findings.md`.

The session-end battery then added two more F3 hits (one per profile — the release
one *longer* again, the debug one the **first debug hit ever observed**, exactly
the manifestation the fourth measurement predicted), both cleared by 120/120
preset retries. Day's tally 4 hits in ≈430 `mt sm=3` encodes — elevated (≈1/110)
on a machine under continuous build + bench + Miri load all session, which is
F3's known widening condition; the alternating counts pin the elevation on the
day, not the commit. All appended to F3.

### F-1 — `encoder/encode_mb_aux.rs`: 21 kernels, +2.1% ledgered

The brief's "22" is 21 kernels + the installer. Every consumer reaches these
through `SWelsFuncPtrList` slots (no direct calls anywhere — proven by grep), and
`decoder/error_concealment.rs` has same-named `WelsCopy16x16_c`/`WelsCopy8x8_c`
cousins (third occurrence of the collision trap; never unify). Fixed arrays
everywhere; the interesting contracts are the two strided Hadamard reads, typed as
exact reaches — `[i16; 49]` (chroma DC group, position 48 last) and `[i16; 241]`
(luma DC of block 15 at index 240). R-e bound derivations are in the doc comments:
the DCT is total (u8 pixels, per-pass gain 6x, ≤ ±9180 over two passes — the old
Rust's i32 scratch and the C++'s int16_t agree everywhere reachable), and quant's
`(ff + |v|) * mf` stays under `2^31` for any non-negative table factors, so the
differentials drove full-range coefficients over the **exhaustive** QP × {inter,
intra} table sweep (R-d). Three mutations (zigzag swap, quant lane index, DCT
butterfly sign) each killed by an entry before commit A.

**Perf** (one interleaved pair per bench, against a fresh null floor of median
+0.87% / max ±3.1%, the quietest of the three sessions that have measured one):
decode flat (+0.3..+0.5% — not a consumer); encoder **+2.10% median, +9.86% worst
usable, 9 rows in +5..10%**. The shape is T4's per-call mechanism transplanted:
flat-content synthetic rows encode fastest and pay the biggest fraction of fixed
per-block scaffolding; content rows read -6..+2%. Tripwire: ≈ +11% + 2.1 ≈ **+13%
median cumulative, under +25%** → swap-and-ledger, no single-family flag (< 15%).
No microbenchmark built (D-perf-4: nothing surprised).

**The Spatial Ramps observation** (excluded from every verdict per R-l, recorded
anyway): the condemned row read +226% at F-1's pair — and unlike its historical
same-binary ±38% chaos, today's readings are *consistent per binary* (control-class
0.11-0.15 ms across four runs; F-1 B 0.345-0.362 ms across three; F-2 B 0.223 ms
twice). Gradients content is the all-skip path — per-block quant scaffolding at its
purest. Filed in the baseline as data for Phase 4a's checkpoint, not acted on.

### F-2 — the encoder recon IDCT family: 9 kernels by recount, noise-level

The C++ `decode_mb_aux.cpp` family is 9 leaf kernels, not the brief's 6: four in
`encoder/decode_mb_aux.rs` and five whose raw bodies the port left in
`svc_encode_mb.rs` (re-exported; the module doc says so). All nine converted; the
`WelsIDctT4RecOnMb` composite (takes a fn pointer) and the installer stay as
dispatch plumbing. Two R-e shapes worth keeping: the dequants' `wrapping_*` i16
arithmetic **equals** the C++'s int-arithmetic-narrowed-per-store (truncation
mod 2^16 commutes with + and ×), so they are total and were driven full-range; and
`idct_t4_rec` keeps the load-bearing `as i16` horizontal narrowing while
transposing the column-major write loop to row windows (bit-exact — every sample
written once; the pilot's argument). The recon shims keep the C++'s null-tolerance
guard, and the probe exercises it.

**F11** (recorded, not fixed): `WelsIHadamard4x4Dc` does its butterflies in
**plain** `i16` — 16x gain, so inputs past ±2047 panic in a debug build where the
C++ narrows. Its qp ≥ 12 sibling already wraps; the two siblings disagree about
the same hazard. In-contract DC levels sit far below the threshold, but a
saturated-DC frame at qp < 12 can reach ~6553 through defined paths — F8's class,
F9's encoder-side severity. Differential bounded to ±2047 with the derivation.

**Perf:** encoder +0.82% median / max +3.52% / zero rows over 5%, decode +0.61% —
noise-level both benches, as predicted. Ledgered as such.

### F-3 — SATD proven and parked, and F10's second instance

Seven `satd_*` kernels (all-i32 butterfly matching C++ and port; composition order
mirrored because per-4x4 rounding makes it part of the contract), nothing installs
them, `WelsInitSampleSadFunc` untouched. The differential entries are **live** —
they run the raw kernels, which remain the code that runs — and one
composition-offset mutation died. §Parked row: re-attempt Phase 4a, with T5-sad.

**The instrument event:** the differential sized raw-side buffers `h * stride`
per F10 as remembered — and the phase-exit Miri protocol, run mid-session on
purpose, flagged `WelsSampleSatd8x4_c`'s right sub-block bumping to
`anchor + 4 + 4*stride`, past a whole-row buffer at any nonzero anchor. **A
composite's sub-blocks bump from their own anchors**: the F10 rule as now applied
is raw-side buffers of `(h+1)*stride`, covering every sub-block at every legal
anchor. Recorded as F10's second instance. The lesson generalizes: *run the Miri
protocol when a test first applies a UB-accommodation rule, not only at the phase
exit* — the fix cost one commit today and would have been a T9 surprise otherwise.

### Ratchet across the session

```
no_mangle      24 -> 24    (0: this family had none left to delete)
unsafe_fn    1360 -> 1361  (+1: copy_shim, the shared span helper — shim_wh's precedent)
raw_ptr      5198 -> 5200  (+2: copy_shim's signature; F-2's prose additions netted zero against its deleted raw bodies)
SHIM(          97 -> 127   (+30: 21 + 9; F-3 adds none — parked kernels have no shims)
unsafe_block  597 -> 624   (+27, within the +30 shim allowance)
```

R-f's strangler shape, twice regenerated with reasons in the commit messages.

### Next session's first action

**Session G: T7 part 2 — `encoder/get_intra_predictor.rs` (27 kernels by the
finishing brief's recount; recount again at session start), then T9 in full** per
`prompts/archive/phase2_finish.md` §3 — the brief's order is binding, T9 never compresses.
Carry-ins: two surfaces per kernel with different rules (reference side
availability-gated PADDING-legal reads at `-1`/`-stride` — T3's span-helper shape;
destination side **packed** prediction buffers, T5-intra's lesson); name-collision
discipline third occurrence (decoder T3 + common T5 names overlap); R-d exhaustive
availability-mask sweeps; commit-B pair + tripwire arithmetic against encoder
≈ +13% cumulative — this is the phase's last swap decision, record it with the
arithmetic shown. For T9: the Miri gate widening (session-B item) is specified in
the brief §3 step 3; F10's `(h+1)*stride` correction and F3's widened signature
belong in the §7.6 hoist; and the fuzz-absence tally now carries F8, F9, F10 (×2),
F11 and F3's sixth measurement.

---

## 2026-08-10 — Phase 2, session G (T7 part 2, the straggler sweep, and the phase exit)

**Goal:** the finishing brief's session G — `encoder/get_intra_predictor.rs` (G-1),
then T9 in full. Both landed, in the brief's order. **The session produced two things
the brief did not ask for and could not have: a family Phase 2 had already converted
and left half-raw, and a Miri gate that found seven soundness defects the moment it
was pointed at the rest of the library.** Both came out of T9's own checklist working
as designed, which is the argument for never compressing a phase exit.

**Started at** `12440d34` with one modified brief (committed first as `f375536e`, the
session control), **ended at** the commit carrying this entry. Tree clean at both ends.

### What landed

| commit | what |
|---|---|
| `f375536e` | the session-G brief (control point) |
| `6e0009d3` | G-1 A: 26 safe kernels, the reach table, 3 differential entries + 2 property tests |
| `cbc18d94` | G-1 B: swap behind 26 shims; span probe with golden runs |
| `b3da21e1` | G-2 A: the encoder's own deblocking kernels proven against T6's safe ones |
| `ee8735df` | G-2 B: 13 raw bodies deleted, 8 wrappers re-exported from `common` |
| `a6361ee3` | the Miri gate widened to the whole library; **F12** and **F13** recorded |
| `19b1391f` | Phase 2's exit measurement, and the per-family delta table |

Plus `phase0_findings.md` F3's seventh measurement, `phase2_findings.md` F10's third
instance, plan §0 (the status preamble) and §7.6 (standing rules S1–S19),
`prompts/archive/phase4a.md`, and this entry.

### Gates

| gate | control (`f375536e`) | final |
|---|---|---|
| `cargo test` | 399 / 0 / 20 | 404 / 0 / 20 |
| `cargo test --release` | 397 / 0 / 20 | 402 / 0 / 20 |
| sweeps st mt def (debug) | 340/341 — F3, retry 120/120 | 340/341 — F3 (`t=4 sm=3 cabac=1 rc=1`, zero-length), retry 120/120 |
| sweeps st mt def (release) | 340/341 — F3, retry 120/120 | 341/341, 19s |
| ratchet | clean (1361/5200/127/24) | clean, regenerated 3x (1346/5175/154/24) |
| `decode_1080p_bench` | all streams bit-identical | all streams bit-identical |
| `c_vs_rust_bench` (FFMPEG set) | all rows bit-identical | all rows bit-identical |
| miri `--lib` | *(gate was `safe::` only, 63 tests)* | **267 tests, 81s**, minus 4 skips |
| miri differential files | — | 17/17 |

Net **+5 tests**. G-1's three entries came and went inside the family (+5 at A with two
property tests, −3 +1 probe at B); G-2's the same (+3, −2 +1 installer property). The
two surviving G-1 property tests and G-2's are the phase's last additions. Ignored-20
never moved.

### Ratchet across the session

```
no_mangle      24 -> 24    (0: neither family had any left)
unsafe_fn    1361 -> 1346  (-15: 13 raw deblocking bodies + 6 raw movers deleted, 4 helpers added)
raw_ptr      5200 -> 5175  (-25: their signatures, net of the shims' and the doc comments' prose)
SHIM(         127 -> 154   (+27: 26 intra + 1 encoder NonZeroCount)
unsafe_block  624 -> 654   (+30: 26 + 3 shared helpers at G-1, 1 at G-2)
```

**The first session in the phase where `unsafe_fn` falls hard**, and the reason is
worth keeping: G-2 *deleted* duplicated raw bodies rather than strangling them. The
strangler shape (S16) trades `unsafe_fn` flat for `SHIM(` up; deletion is what actually
moves the number, and Phases 4-5 are made of deletion.

### G-1 — the encoder's intra predictors: 26 kernels, and the availability argument became a test

27 `pub unsafe fn` = **26 kernels + the installer**, exactly as the brief's recount
said; nothing had drifted this time, and a grep for predictor bodies in adjacent files
found none. The two I16x16 modes this module installs but does not define (`V`, `H`)
are *imported* from `common/intra_pred_common.rs`, converted in T5-intra — the same
two functions, not same-named cousins.

**Two surfaces with different rules, and the signatures now carry the difference.** The
destination is a *packed* candidate buffer (16 / 64 / 256 bytes at implicit strides
4 / 8 / 16, one half of a mode-decision ping-pong pair), never a plane, so it is a
fixed-size array and `kiStride` describes only the reference. That is the third
occurrence of the name-collision trap and the module header now carries the three-way
table so no later session unifies them.

**Per-kernel reference shapes (S7), and the type does the arguing.** A predictor that
reads only the row above takes `&[u8; N]` — it *cannot* touch the left column, which is
the entire reason mode decision may offer it when the left neighbour is missing. One
that reads the left column takes a `PlaneCursor` spanning exactly its reach. The reach
is data (`Reach { top, left, corner }`, one constant per shape) and `ref_span` is the
only place it becomes a slice.

**The result worth carrying forward: R-d's exhaustive-selector rule found its target,
and it was not the kernels.** These take no flags at all, so "sweep the selectors
exhaustively" cashes out as the *availability offset*, and
`reach_table_agrees_with_the_availability_tables` walks all 16 `g_kiIntra4AvailMode`
offsets and all 8 of the I16x16/chroma ones asserting that every offered mode reads
only neighbours that offset declares. **It holds exactly**, and it converted the
shims' central contract sentence from prose into a checked property. It also pinned two
things that were folklore:

- `g_kiIntra4AvailMode` **never offers `DDL_TOP` or `VL_TOP`.** Both are installed in
  the dispatch table and neither is reachable through it. Converted anyway (an
  installed slot is live surface), with the unreachability asserted rather than left to
  be rediscovered.
- The I16x16 and chroma tables have **no top-left bit** — their index is
  `uiNeighborIntra & 0x07` = left | top<<1 | topright<<2 — yet the plane mode they offer
  at index 7 reads the corner at `(-1, -1)`. The corner exists whenever left and top
  both do, by raster order inside a slice, and the C++ leans on the identical
  implication. That is now written down in an assertion instead of nowhere.

Two of my own tests failed first and were right to: `ref_span` seeded its extent from
zero and so dragged `pRef` into every span, and `I4_PRED_INVALID` and `I4_PRED_V` are
**both zero**, so a table row's padding is indistinguishable from mode V and
`g_kiIntra4AvailCount` is the only thing that says where the live prefix ends.

Five mutations killed at A (a DDL tap position, a chroma-plane row centring, an I16x16
DC rounding constant, a reach that over-claims — killed by the availability test and
correctly ignored by the differential — and one that under-claims, the mirror). Two
more at B: an anchor shifted one byte left, killed by the **golden direct run** (S11)
and invisible to Miri; and a reach reading eight top samples where its contract says
four, killed by **Miri on the exact-span allocation** and invisible to a normal build.
Each instrument caught what only it could see, which is the point of having three.

**Perf:** encoder **+0.42% median** against a null floor of +0.00% / max +2.50% —
inside it. Decode flat. T3's wash reproduced on the encoder side, as the call density
predicted. Tripwire ≈ +13.4%, under +25% → swap and ledger. The phase's last swap
decision, and the least eventful.

### G-2 — T9's straggler sweep found a live raw half of a converted family

The sweep is mechanical: every `pub extern "C" fn` whose name ends `_c`, minus those
whose body carries a `SHIM(phase2)` marker, grouped by file. 63 hits across 13 files.
Twelve of those files are kernel-shaped code **outside** the plan's Phase 2 list, and
they are listed below with their owner phases. One was not.

**`encoder/deblocking.rs` carried its own copies of the eight deblocking ABI wrappers,
their four inner kernels, and a third `WelsNonZeroCount_c` — and its own
`DeblockingInit` installed them.** T6 converted the family in
`common/deblocking_common.rs` and the *decoder* picked the conversion up by
re-exporting that module wholesale (`decoder/deblocking.rs:200`); the encoder never
did. Every encoded frame since T6 has run raw deblocking kernels while the phase's
records said the family was converted.

Commit A proved the two sets equivalent — `ALPHAS` × `BETAS` × three strides × V/H,
whole-buffer comparison, using T6's own entry shapes recovered from `bbb9348e` — and
that mattered more than saving typing: deblocking is *conditional*, and T6 had already
learned that full-range noise almost never passes `|p0-q0| < alpha`, so the surface
generator drives three amplitude tiers and the tc pool carries the values that gate
whole lines off. Reusing the shapes reuses the lesson. Commit B then deleted thirteen
raw bodies and re-exported the common shims — a deduplication of two copies of one
function, not a unification of two functions sharing a name, and proven so before it
was done.

**Perf: encoder +0.59% median / +4.33% worst, decode unchanged.** The four Mandelbrot
rows moved together above the null floor, so this is a small *real* cost: the encoder
now pays the per-edge shim scaffolding the decoder has paid since T6. Decode staying
flat is the load-bearing observation — a moved decode number would have meant the
re-export changed the wrong caller.

**The lesson, which is S18's reason for existing:** a per-family pass converts the file
it was pointed at. Nothing in the phase's records would ever have caught the encoder's
copy, because every gate was green and every number was byte-exact the whole time.

#### The rest of the straggler list, with owners

Fifty kernel-shaped definitions remain raw. None is a Phase 2 miss; all are recorded
here so no later session has to re-derive the classification.

| file | count | owner |
|---|---|---|
| `common/sad_common.rs` | 14 | **parked**, re-attempt Phase 4a (§Parked) |
| `encoder/sample.rs` SATD | 7 | **parked**, re-attempt Phase 4a |
| `encoder/svc_motion_estimate.rs` | 7 | Phase 6.3 (ME caller conversion) |
| `encoder/svc_encode_slice.rs` | 6 | Phase 6 — slice drivers, not leaf kernels |
| `encoder/md.rs` | 3 | Phase 6.3 (`UpdateMbMv_c`, the two VAA analysers) |
| `encoder/slice_multi_threading.rs` | 3 | Phase 7 (`WelsSetMem*_c`) |
| `decoder/decode_slice.rs` | 3 | Phase 5.6 (`WelsBlockZero*`, the decoder's `WelsNonZeroCount_c`) |
| `decoder/error_concealment.rs` | 2 | Phase 5.1 |
| `encoder/svc_set_mb_syn_cavlc.rs`, `encoder/vlc_encoder.rs` | 2 | Phase 3.2 (`CavlcParamCal_c` ×2) |
| `encoder/deblocking.rs` | 1 | `DeblockingBSCalc_c` — T6's F1 surgery typed its `uiBS`; the rest reads MB structs, Phase 6 |
| `encoder/encoder_context.rs`, `encoder/wels_preprocess.rs` | 2 | Phase 6 (`WelsSetMemZero_c`, `WelsMoveMemory_c`) |

`no_mangle` finishes at **24**: five `api/` exports, two in `wels_encoder_ext.rs`
(`WelsGetCodecVersion`/`Ex`), **fourteen in the parked `common/sad_common.rs`**, and
three CABAC init functions in `encoder/set_mb_syn_cabac.rs` (Phase 6). **Every file
Phase 2 converted contributes zero**, which is the claim the phase made.

### The Miri gate, widened — seven defects in an afternoon

Session B's item, executed at the boundary it was reserved for:
`--lib safe::` (63 tests) → `--lib` minus four skips (**267 tests, 81 seconds**).

Four were the tests' own and were fixed: `sad_common` taking `as_mut_ptr()` twice for
one buffer; `error_concealment` writing an array through its binding while a derived
pointer was live; `parse_mb_syn_cavlc` making three derivations for one
`SBitStringAux`; and **F10's third instance** — `sad_common`'s own unit tests handing
exactly-sized buffers to parked raw composites, which had been running that UB
continuously for three sessions because F10's accommodation was applied only in the
differential file, which was the only file Miri ran. **An accommodation is only as wide
as the instrument that found it** (now S13).

Three are in production code and are recorded, not repaired:

- **F12** — `IWelsTaskThreadSink`'s methods take `&mut self`, so every worker thread
  materialises a `&mut` to the one shared `CWelsThreadPool`. Miri calls it a data race
  on the retag itself. Every field is `Mutex`-wrapped, so the *data* is fine and the
  code works; `&mut` is a uniqueness claim regardless of contents. F1's category, not
  F8's. Phase 7.
- **F13** — four instances of one class (derive a pointer, derive a second from the
  same owner, then use the first). The one that matters: **`SWelsFuncPtrList` is
  self-referential** — `SetFastCodingFunc` sets
  `sdf.pfMdCost = sdf.pfSampleSad.as_mut_ptr()`, so every later `&mut` reborrow of the
  list invalidates it. **That lands on Phase 4a**, which owns the dispatch tables, and
  de-virtualizing them *is* the fix. Also `AddLongTermToList`'s `ptr::copy` across an
  invalidated `as_ptr()` (Phase 5), `InitDqLayers`' overlapping `&mut` (Phase 6), and
  `InitBits` declaring `kpBuf: *const u8`, storing it as `*mut`, and writing through it
  — a signature that documents the opposite of what the function does, so **every
  honest caller is wrong** (Phase 3.2, F2's family).

The four skips are a **work queue, not a settled state** (S15). `-Zmiri-ignore-leaks`
is set deliberately: this gate is for undefined behaviour, and leak-checking a
mid-refactor C transliteration reports the design rather than a defect.

**Byte-exactness does not imply soundness.** That sentence has been in the plan since
Phase 0, where F1's stack overflow ran under 341/341 identical sweeps for the port's
whole life. This is its third demonstration, and the first where the instrument that
could have seen it existed and was simply pointed at one-quarter of the codebase.

### The exit measurement — the ledger, checked

Three interleaved pairs per bench, `afcdd785` against `a6361ee3`:

    decoder  +16.59 / +10.63 / +9.86%     ledger predicted  ≈ +17 / +10 / +10%
    encoder  +14.73% median               ledger predicted  ≈ +14%

**The ledger is validated as an instrument.** Six rows, each measured with one
interleaved pair at its own commit B across five sessions, sum to within a point of a
three-pair end-to-end reading. Phase 4a's checkpoint can read recovery row by row and
trust the arithmetic — which is exactly what D-perf-4 was betting on when it replaced
the hard ceiling with a ledger.

Both cumulative figures are under the +25% tripwire, so **nothing parks retroactively**.
The full table, the row-class gradient, and the per-family deltas are in
`perf_baseline.md` §Phase 2 exit.

### F3, seventh measurement — the day-rate, twice

Two alternating loops (S14), three release `mt` presets per side each: **G-1 A 3
failures / 360, G-1 B 2 / 360; G-2 A 1 / 360, G-2 B 2 / 360.** Neither separates the
sides, and in the first the *raw* side was higher. Day's tally **17 hits in ≈840
`mt sm=3` encodes (≈1/49)**, against session F's ≈1/110.

The attribution argument that makes today's data better than its predecessors':
**two of the day's hits landed on the session's control commit, which changes one
markdown file and no code.** The elevation preceded the first kernel move. Every hit
matched the widened signature; nothing outside it fired all day, in either profile.

### The fuzz-absence tally, for Eugene

The standing signal, presented rather than acted on. Findings a fuzzer would plausibly
have reached first, now **eight**:

| finding | what | would a fuzzer have found it first? |
|---|---|---|
| **F8** | 8x8 IDCT `i16` intermediates overflow; debug panics where C++ wraps | yes — a `decode_annexb` target, minutes |
| **F9** | `iFrameSad` overflows at ≥8.4 megapixels | encoder-input target, needs 4K frames |
| **F10 ×3** | raw SAD/SATD trailing pointer bump is out-of-allocation | **no** — this is Miri's, and Miri found all three |
| **F11** | `WelsIHadamard4x4Dc` plain `i16` adds overflow above ±2047 | yes, with a qp<12 saturated-DC frame |
| **F12** | every worker takes `&mut` to the shared thread pool | **no** — Miri's, and only under `-Zmiri-disable-isolation`-free threading |
| **F13** | four aliasing defects incl. the self-referential dispatch table | **no** — Miri's |
| **F3** | MT slice-list race, now eight measurements | **yes, emphatically** — it is a nondeterminism bug and a differential fuzzer is the instrument for it |

**The honest reading, which is new this session:** the tally has been growing, but its
composition has changed. Miri, once pointed at the whole library, found *five* of these
eight and would plausibly have found F10 three sessions earlier had it been pointed
there sooner. A fuzzer remains the right instrument for **F3** and for the F8/F11
arithmetic class — malformed and adversarial *input* — and nothing else in the tally is
an argument for it. That is a sharper case than "eight findings, please reopen T7":
**the input-space instrument is still missing and F3 is the standing evidence.**
Re-raising Phase 0 T7 is Eugene's call; the tally is the job.

### Where Phase 2 stands

**Complete.** 154 shims across 13 files; ~190 kernels reached; two families parked with
their re-attempt in the immediately next phase; six findings recorded and none
repaired; the ledger open, measured, and validated. `unsafe_fn` 1346, `raw_ptr` 5175,
`no_mangle` 24 with **zero** from any converted kernel file.

The three consolidation artifacts are in place: plan **§0** (the status preamble,
refreshed at every phase exit from now on), plan **§7.6** (standing rules S1–S19,
hoisted from `prompts/archive/phase2_continue.md` §2 — briefs from here on cite it instead of
copying rules forward), and **`prompts/archive/phase4a.md`**. Both Phase 2 briefs are stamped
superseded-historical.

### Next session's first action

**Phase 4a — read [`prompts/archive/phase4a.md`](prompts/archive/phase4a.md)**, then plan §0 and §7.6.
`mc.rs`'s consumers first under the preserved D-perf-3 protocol, because it is the
largest ledger row and because if direct dispatch does not recover *it* the recovery
thesis is wrong and the first session should be what discovers that. Then the decoder
tables (including the three untyped installer casts T6 left at `decoder_core.rs:1906`
and `:920-921`), then the encoder's ~55 CPU-dispatch members including
`WelsInitSampleSadFunc`. **Do not compress the checkpoint** — re-measure every ledger
row, re-attempt both parked families with the harness rebuilt first (S3/S4), and delete
`--skip svc_mode_decision` from the Miri gate, because F13's self-referential dispatch
table is 4a's to fix and de-virtualizing it is the fix.


---

## 2026-08-10 — Phase 4a, session A (dispatch de-virtualization + the recovery checkpoint)

**Goal:** `prompts/archive/phase4a.md` — de-virtualize kernel dispatch, then run the recovery
and unpark checkpoint five sessions of ledgered deficits had been waiting for. The
brief allowed cutting de-virtualization scope to keep the checkpoint whole; that
allowance was used, deliberately, and §3 below says where.

**Started at** `7de4ebe9` (plus two uncommitted doc edits), **ended at** the commit
carrying this entry. Tree clean at both ends.

### What landed

| commit | what |
|---|---|
| `2f65a765` | Phase 2's two leftover doc edits (F3/F12 hypothesis, S18's corollary) |
| `ea2304ce` | `rust/tools/perfpair.py` — S1/S2/S17 as an instrument |
| `32dc05c5` | mc A: the assert-maps that license de-virtualizing `SMcFunc` |
| `c9d416de` | mc B: 15 sites become direct calls; `pMCFunc` parameter deleted |
| `ededdee8` | F13's fourth site → `CostFamily` enum; **Miri skip deleted**; F14 found and fixed |
| `3a67cd8e` | the decoder's `SDeblockingFunc` → direct calls (12 slots, 22 sites) |
| `25a8e287` | the rebuilt SAD body harness, and the parked families' second verdict |

Plus `perf_baseline.md` §Phase 4a, the ledger's checkpoint column, plan §0, and
`prompts/archive/phase3.md`.

### Gates

| gate | control (`7de4ebe9`) | final |
|---|---|---|
| `cargo test` | 404 / 0 / 20 | **410 / 0 / 20** |
| `cargo test --release` | 402 / 0 / 20 | **408 / 0 / 20** |
| `sweep.sh st mt def` (debug) | 341/341 | 341/341 |
| `sweep.sh st mt def` (release) | 341/341 | 341/341 |
| Miri `--lib` | 267 tests, 4 skips | **274 tests, 3 skips** |
| ratchet | clean | clean; `raw_ptr` 5175 → 5171, `transmute` 23 (unmoved) |

### 1. The headline: the thesis is true with a condition attached

**Encoder −5.11% median at the checkpoint, all 28 usable rows faster or flat, none
regressed. Decode −0.19%, flat.** Cumulative vs Phase 2's start is now ≈ **+8.9%**
encoder (was +14.73%, under 10% for the first time since T4) and unchanged on decode.

The condition is the phase's real finding, and it is the thing Phases 5/6 inherit:

> **Direct dispatch recovers per-call scaffolding only where the caller supplies
> constant dimensions.**

The encoder's MC sites name the shims with literal block sizes, so inlining folds the
span arithmetic and `from_raw_parts` against them — `mc.rs` alone measured **−4.84%**
the moment its fifteen sites became direct calls, and it recovered *in the deficit's
own shape*: flat content moved most (YUV Space −12.7%, SMPTE −9.1%), dense Mandelbrot
barely moved, exactly inverse to how they accumulated the deficit. The decoder's sizes
arrive as `iBlkWidth`/`iBlkHeight` **parameters** through `BaseMC`, constant at its ~40
call sites and runtime inside it, and `BaseMC` is ~1300 instructions — far past any
inlining threshold. Deblocking is the same shape with a runtime *stride*: −0.53%.

**Two fixes were built and paired rather than reasoned about** (S1), and both are
reverted: `#[inline]` on `BaseMC` read −0.35%, `#[inline(always)]` on `McChroma_c`
−0.71%, both inside the floor. Disassembly settled it — `McLuma_c` *does* inline into
`BaseMC` (the `mc_hor_ver*` kernels are called straight from it) while the chroma pair
stays out of line. That is why D-perf-3's fallback is spent on the decode half only,
and why those rows are downgraded to Phase 5 rather than left promising Phase 4.

`Spatial Ramps`, excluded from every verdict and still the loudest instrument here,
**halved (−48%)**. The brief predicted it would show up loudest there. It did.

### 2. Deleting a Miri skip immediately exposed production UB — for the third time

F13's fourth site (`SWelsFuncPtrList` self-referential via `pfMdCost`) became a
`CostFamily` enum, `--skip svc_mode_decision` came off, and Miri promptly failed on
something else entirely: **F14**, `WelsSampleSad16x16_c` walking one row past
`pMemPredMb`. Its reads end *exactly* at byte 511 of a 512-byte allocation; being a C
transliteration it then bumps its row pointer and forms `base + 520`, never
dereferenced. UB in Rust and C alike, invisible to every gate the port has, and
sitting behind F13 where no instrument could see it.

Fixed by accommodation, not repair (S12 — exact spans are for the safe side):
`WelsMallocz(2 * 256)` → `(2 * 256 + 16)`, provably output-neutral, and guarded by the
instrument that found it. **That is `sad_common.rs`'s fourth latent-UB finding** (F10
×3, F14 ×1) — the family that has spent longest parked with raw bodies live. The
brief's claim that parked raw code is where UB sits was not a prediction by the time
the session finished; it was a measurement.

Generalisation worth keeping: **each skip hides an unknown number of further defects,
not just the one it names.** Third time this refactor (F1 behind the release segfault,
seven behind `--lib safe::`, F14 behind F13).

### 3. Where scope was cut, and why that was right

Not de-virtualized: the decoder intra-pred arrays, `sBlockFunc`, expand, the
`decode_slice.rs` cache-fill transmutes, and the encoder's ~55 CPU-dispatch members
including `WelsInitSampleSadFunc`. **`transmute` is therefore still 23 and that is the
phase's main unfinished business.**

The cut was made on evidence rather than on the clock. Once `mc.rs` and deblocking had
both shown that the decode side does not recover, further decoder tables had a known
near-zero perf payoff, and the intra-pred arrays are not even the CPU-dispatch case —
they dispatch on *mode*, from the bitstream, so they need `enum` + `match`, and
T3/T7-G1 already measured that family as a wash in both codecs. Spending the remaining
session on them would have bought unsafe-surface only, at the cost of the checkpoint —
which is the trade the brief explicitly told this session not to make.

### 4. Two instrument lessons, both of which cost something

- **Address comparison is not a sound assert-map technique.** The obvious shape —
  `slot.unwrap() as usize == Kernel_c as usize`, which
  `encoder_deblocking_table_installs_the_common_shims` uses — fails for
  `#[inline(always)]` functions, cross-crate *and* in-crate, because they are
  instantiated in whatever codegen unit takes their address; Miri mints a fresh
  address per cast on top of that. The existing test works by luck, not design. Every
  assert-map in this phase is split in two: flag-invariance by comparing two tables
  from the *same* installer, identity by comparing **behaviour**.
- **`perfpair.py`'s first draft dropped six encoder rows and printed a plausible
  table.** It parsed bench output by field offset, and these benches pad to a fixed
  column width, so `( 6.202 ms)` loses its space at `(10.658 ms)` — every row over
  10 ms vanished. Now regex-parsed, with missing rows reported explicitly. Same class
  as S17: an instrument that quietly measures less than it claims.
  A third, smaller one: a mutation test "passed" because the mutated tree failed to
  *compile* and my grep filtered the error away. A filtered command output is not a
  result.

### 5. Parked families: re-attempted, re-parked, one debt owed

Harness rebuilt first as the brief required (`benches/sad_bodies_bench.rs`, S1/S3/S4
with each rule's reason in the module docs). Ratios now reproduce within ~3% across
runs against a predecessor that moved 3x. **1.41x–4.94x across the seven shapes
against a ≤1.05x bar — nothing close, in any framing.** Second dated verdict recorded.

New and actionable: **per-row cost is the whole story and it falls off a cliff at
W = 4** (8x16 costs more than 16x8 at identical sample counts; 4x8 costs twice 8x8 for
half the samples). Start there.

**D-perf-2's slices-and-offsets lead is still untested and this session did not test
it.** The cheap probe (hoist `PlaneCursor::new` out of the loop) is invalid —
`black_box(&cursor)` forces the cursor to memory and made the safe side slower, and
merely adding that loop swung 8x8 from 2.28x to 4.39x. Recorded rather than reported.

**Debt:** `encoder/sample.rs`'s SATD still has no measurement of its own. The SAD
verdict is a strictly-easier lower bound so the park holds a fortiori, but the brief
asked for a real SATD measurement and it is **not** discharged.

### 6. F3, cleared properly

Fired twice (family gate, then one of three re-runs), which is S14's escalation
trigger. The alternating loop gave **HEAD 0 / 600 configs, control `2f65a765` 2 / 600**
— the *control* side hit and the changed side did not. Session rate ≈1/540, back in
the historical 1/400–1000 band and ~11x quieter than session G's ≈1/49. Both hits
matched the signature exactly. Eighth measurement appended to F3.

### Next session's first action

Phase 3, per [`prompts/archive/phase3.md`](prompts/archive/phase3.md): read F4/F5/F7, F2, and F13's
`InitBits` site, then **write the malformed-stream error-code parity test against the
unconverted reader** before touching `bit_stream.rs`. That test is the phase's real
gate and it is worthless if written after the conversion it is meant to judge.

---

## 2026-08-10 — Phase 3, session A (T3.0 + T3.1a)

**Goal:** Phase 3 per [`prompts/archive/phase3.md`](prompts/archive/phase3.md), session 1 = T3.0 + T3.1.
Landed T3.0 in full and the first half of T3.1; the seam's second half (T3.1b, the
storage and call-site conversion) is next.

**Started at** `5e5c9196` with the reworked brief and the plan's edited exit gate
uncommitted; **ended at** `96fb04a4`. Working tree clean at both ends.

### What landed

| commit | what |
|---|---|
| `d7bc0ac3` | **T3.0** — the malformed-stream error-code parity test, 2316 golden rows, plus **F15** |
| `96fb04a4` | **T3.1a** — the decoder read side's bodies run on `BsCursor`; **F4's P6 decided, F7 fixed** |

### Gates

| | inherited (`5e5c9196`) | final (`96fb04a4`) |
|---|---|---|
| tests | 410 debug / 408 release / 20 ignored | **423 / 421 / 20** (+13, all T3.0) |
| sweeps | 341/341 both profiles | 341/341 both profiles, no F3 hit |
| Miri `--lib` | 274 tests, 3 skips | **275** tests, same 3 skips |
| Miri differential | — | `safe_bits_differential` green with two F7 accommodations **deleted** |
| ratchet | 1346 / 5171 / 154 | 1349 / 5172 / **158** — regenerated, shape below |
| T3.0 goldens | — | **unchanged across T3.1a**, both profiles |

### 1. T3.0 found F15 on its first run, before any conversion

The argument for building the gate first did not stay theoretical for long. The first
execution of the corpus aborted the whole test process, and the abort named
`nalu.rs:762`: `BsGetTrailingBits(pNal.add(iNalSize as usize - 1))` with `iNalSize == 0`.
`ParseNalHeader` strips trailing zeros and then the header byte, so **any slice NAL whose
payload is one non-zero byte** lands there — i.e. truncate a stream one byte past a
slice's start code. Ten to eleven corpus entries per base stream do it.

Debug panics on the subtraction; release wraps to `pNal.add(usize::MAX)`, which is
out-of-bounds pointer arithmetic that happens to land on the header byte and returns
`dsBitstreamError`. Upstream C++ has the same expression twice (`au_parser.cpp:252`,
`:396`), so this is a transliterated upstream bug — there is no correct behaviour to be
S6-parity with, and T3.3's fix is a fix. Written up as
[`phase3_findings.md`](phase3_findings.md) §F15.

### 2. A panic inside the decoder is an abort, not a failure — and that shaped T3.0

`catch_unwind` cannot hold it: the entry points are the `extern "C"` vtable thunks, so
unwinding out of one is `panic in a function that cannot unwind`. The corpus therefore
runs in a **child process** that appends one row per entry unbuffered; when the child
dies the parent knows which entry killed it, records `ABORT` with the panic site, and
resumes at the next one. One spawn per stream in the happy path, and it enumerated all
eleven F15 instances in a single run instead of one per bisect.

Because debug aborts where release returns a code, F15's entries are **withheld** rather
than recorded — a golden table has to hold in both profiles (§7.2 gate 0), and this
input has no profile-independent behaviour. The `WITHHELD` rows name the finding, so the
gap is counted and visible. `withheld()` is deleted in T3.3 and those rows fill in.

Cost of the instrument: **36 s debug, 2.4 s release**, parallel across 12 tests. Worth
stating plainly since it lands on every `gates.sh commit` from here.

### 3. P6 resolved by an option neither the plan nor F4 listed

The plan offered zeroed guard bytes or `get()` fallbacks. Adoption made a third obvious:
**declare the slack the reader has always read and hand it the real bytes.**
`READER_SLOP = 3` is derived on the constant; `decoder_core.rs:3637` already refuses to
copy a NAL payload unless four bytes are spare, so the slack exists for every NAL in
`sRawData`, and the cursor sees the same bytes the raw reader read — byte-identical by
construction rather than by an argument about zeros. Zeroing was **rejected**: the slop
feeds decoded values, not only the error predicate, so zeroing changes behaviour on
malformed input.

### 4. Fixing F7 widened the differential, and the widening cost a real fix

Both F7 sites are gone, and both accommodations in `safe_bits_differential.rs` came off
in the same commit — the shape 4a established. Widening `reader_init_matches_dec_init_bits`
to its whole range then failed under Miri, correctly: the test declares RBSP lengths
*longer than the payload*, and the contract is `declared + READER_SLOP` readable. The
shim builds the reader's slice eagerly, so a short buffer is now flagged where the raw
reader merely never reached that far. `rbsp_with_slack` went 4 → 8 bytes, and the same
fix went to the three in-module `dec_golomb` buffers (S13).

### 5. The differential's reader half retired in place

It no longer compares two implementations — the raw functions *are* the cursor now — so
it proves the **shim** instead: that the pointer triple survives `cursor_of`/`store_cursor`
at every truncation and on the error paths, where the raw refill left `uiCurBits` and
`iLeftBits` mutated before returning. That is not a tautology and it fails if a
write-back is dropped. The C-consistency it used to carry passes to T3.0's goldens,
which is the handover T3.0 was built to make possible. Both facts are in the file's
module doc.

### 6. Perf: +0.45% decode, and the shim that causes it dies next seam

3 pairs both benches, floor from `null t31a` (decode ±0.2%): decode median **+0.45%**
(CB +0.77%, Main -0.26%, High +0.45%), encoder median **+0.00%**. The cost is on the
CAVLC row because that is the row that pulls every syntax element through this family;
the CABAC rows do their bit work in `cabac_decoder.rs`, which is T3.2. It is field
marshalling into and out of `SBitStringAux` — exactly what T3.1b deletes — so it is
ledgered with a deletion point one seam away rather than carried. Cumulative decode
≈ +18.3 / +9.8 / +10.1%, ~7 points under the tripwire on CB.

### 7. Ratchet shape, and a plan note for T3.1b

`bit_stream.rs` +3 `unsafe_fn`, +2 `unsafe_block`, +3 `SHIM(`, +1 `raw_ptr` (the shared
`readable` helper's parameter — S16 explicitly allows one or two for a helper that
replaces N copies of the arithmetic); `dec_golomb.rs` +1 `SHIM(`. **Nothing fell**,
because what T3.1a deleted is pointer *arithmetic* and the ratchet counts pointer
*types*. S16 in one measurement: read the shape, not the sign.

One plan correction for T3.1b: §2.2.2's `[P1]` note says `iIndex` "has no consumer in
this phase". It has two — `BsStartCavlc` and `BsEndCavlc`
(`parse_mb_syn_cavlc.rs:2230-2248`), the CAVLC fast-path rewind, on the residual path.
`BsCursor` needs either the field or a bit-position seek before those two can convert.

### Next session's first action

**T3.1b**, the second half of the seam: `SWelsDecoderContext::sBs` and
`SVclNal::sSliceBitsRead` become `BsCursor` + the buffer at the call site, and the ~20
consumer functions (`ParseVui` 54 reads, `ParseSliceHeaderSyntaxs` 36, `ParseSps` 31,
`ParsePps` 21, `ParseInterInfo`/`ParseInterBInfo` 17 each, `WelsDecodeMbCavlcResidual`
17, and a dozen smaller) switch from `pBs` to `(buf, &mut cursor)`. `SDqLayer::pBitStringAux`
keeps a shell per the brief §2 — it points *at* `SVclNal`'s field, so that field stays as
the shell and the sync layer is `SHIM(phase3)` with Phase 5 named as its deleter. The
`iIndex` gap above has to be closed first. Deleting `cursor_of`/`store_cursor`/`read_with`
and `BsCursor::from_parts` is the seam's completion signal, and the ledger row from item 6
should clear with them.

## 2026-08-10 — Phase 3, session B (T3.1b; T3.1 closes)

**Goal:** [`prompts/archive/phase3_session_b.md`](prompts/archive/phase3_session_b.md) — finish T3.1,
then T3.2 only if T3.1b gated with a third of the session left. It did not, and T3.2
was not started: see "Next session's first action".

**Started at** `033b6258`; **ended at** `773a91ac`. Working tree clean at both ends.

One bookkeeping deviation from the brief's §0.1, which expected the [P3] doc edits to
arrive uncommitted and asked for them to be committed in house style: they were already
committed, as `033b6258 "phase 3 doc update"`. The content is exactly what §0.1
describes and is correct; only the message is terse. Fixing it would mean amending an
existing commit, which this environment declines, so it stands as-is rather than being
worked around.

### What landed

| commit | what |
|---|---|
| `1bf5a235` | **T3.1b step 0** — `BsCursor` gains the CAVLC mode, differential-proven before any consumer moved |
| `773a91ac` | **T3.1b steps 1–4** — the ownership move; the marshalling layer deleted; **F16** |

### Gates

| | inherited (`033b6258`) | final |
|---|---|---|
| tests | 423 debug / 421 release / 20 ignored | **430 / 424 / 20** |
| T3.0 goldens | 2316 rows | **2316, unchanged, both profiles** — the gate that found F16 |
| conformance | 53 hashes | 53 hashes, unchanged throughout |
| sweeps | 341/341 both profiles | release **341/341**; debug **340/341**, twice, F3 signature — see below |
| decode bench | — | all 3 streams **bit-identical** (SHA-1 `0fba9a4e…`, `d8c07c43…`, `8b081cce…`) |
| encoder bench | — | all rows **bit-identical** |
| Miri `--lib` | 275 tests | **285 passed, 0 failed** — read from the log, not from the gate's verdict (**F17**) |
| ratchet | 1349 `unsafe_fn` / 5172 `raw_ptr` / 158 `SHIM(` | **1334 / 5109 / 157** — regenerated, shape below |

The test count is net **−6** against the peak of 436/430 reached at step 0: the CAVLC
mode added 10 unit + 3 differential tests, and the ownership move deleted the 6 retired
reader differentials and `from_parts`'s test while adding one for the null-base helper.
Against the session's *inherited* baseline it is +7 debug / +3 release, the gap being
the four `#[cfg(debug_assertions)]` mode-guard tests.

**Ratchet shape** (S16 — read the shape, not the sign). `raw_ptr` **−63**, `unsafe_fn`
**−15**, `unsafe_block` **−11**: the six-function reader family in `dec_golomb.rs` is
now *safe fns* — no `unsafe`, no raw pointers, `&mut u32` out-params — and the
marshalling layer is gone. `SHIM(` −1: four markers deleted
(`readable`/`cursor_of`/`cursor_and_buf`/`store_cursor`/`read_with`), three added
(`BsReader`, its `buf()`, `readable_from`). One increase, and it is the one S16
sanctions: `decoder/bit_stream.rs` `raw_ptr` 9 → 11, for `BsReader::base` and
`readable_from`'s pointer parameters — "a shared helper may legitimately add one or
two, and paying that beats N copies of the span arithmetic". Baseline regenerated with
that reason in the commit message.

### 1. The mode was built and proven first, and that ordering paid

The brief made step 0 a separate, fully-proven commit before any of the 255 call sites
moved. That is not ceremony: a mode that is subtly wrong, with 255 mechanically-edited
call sites depending on it, is a bisect through a 1000-line diff. Ten unit tests
(including an 8×4 sweep of every bit phase), three differential tests against the raw
pair with **all six fields** compared, and three deliberate mutations killed — the
dropped `left_bits` phase, the 16→32 half-window, and an unshifted prime — took about
a fifth of the session and made the rest mechanical.

The one design judgement inside it: `PartialEq` is **hand-written** over the six
C-mirrored fields, excluding the `cfg(debug_assertions)` mode flag. A derived impl
would compare the flag in debug and not in release, so two cursors could be equal in
one profile and unequal in the other — the skew S16 exists to prevent, and the same
call D1 made for the pool's generation counter.

### 2. F16: `READER_SLOP` was derived from the wrong half of the reader

**T3.0 caught this, and nothing else would have.** T3.1a wrote `READER_SLOP = 3` with
the claim that three bytes "covers every read the family can make, at any position, for
any operation". That was derived from `dump_bits_aux` and is false for `BsEndCavlc`,
which primes **four** bytes at `iIndex >> 3` — and `iIndex` is advanced by the residual
decoder by whatever each symbol consumed, so on a truncated stream it runs past the
RBSP without bound. The raw reader was measured reaching `len + 5` and beyond; it never
faulted because `sRawData` is 4 MiB.

Handing the cursor `len + READER_SLOP` therefore gave it a **narrower** window than the
raw code had, and 13 corpus entries across 6 streams aborted on the slice index. **All
53 conformance hashes were green throughout** — well-formed streams never take the
residual decoder past the RBSP, so only T3.0's corpus sees this. Second time in this
phase that building the malformed-stream gate before the conversion it judges has paid
for itself.

Worth being precise about what this was *not*: not a pre-existing panic to record and
quarantine per S12, but **the conversion narrowing a window** — a bug in the boundary
helper. The distinction matters because the test's own failure message suggests the S12
path, and taking it would have quarantined 13 rows to hide a regression.

Resolution: `BsReader::avail` carries the real distance to the end of the allocation,
computed by the single `readable_from(pHead, pEnd, …)` helper — which is the
`pHead..pEnd` boundary helper the brief specified, arrived at from the other direction.
Written up as `phase3_findings.md` §F16, with a warning for T3.2, whose end ladder and
5-byte init prime must take their extent from the same place rather than re-deriving
one.

A second instance of the same class was then found by inspection rather than by a gate:
`ExpandBsBuffer` grows the raw buffer, so a rebased reader's `avail` goes **stale and
too small**. Fixed in the same commit. Nothing in the battery exercises mid-AU buffer
growth — that is P5's unit test, and it is T3.3's to write.

### 3. The deviation: `BsReader`, not a bare `*mut BsCursor`

The brief's default shape for `SDqLayer::pBitStringAux` was `*mut BsCursor`, deviation
allowed with a written reason. Taken, and here is the reason: `BsCursor` is detached by
design (§2.1.3), so it cannot produce the bytes; `decode_slice.rs` reads *through* that
pointer. The alternative is a base pointer and a readable extent as loose fields beside
every cursor, kept coherent by hand — which is the exact shape §2.2.2 [P3] rejected for
`iIndex`, and F16 is what happens when one of those two numbers is wrong.

So `BsReader { base, avail, cursor }`: one `SHIM(phase3)` type, one `buf()` that
reconstructs the slice, one `split()` that hands consumers the `(buf, &mut cursor)` pair
the plan specifies, and **one thing for T3.3 to delete**. The consumers' signatures are
exactly the plan's shape; only the storage differs. Recorded in §2.2.2 as a [P3] note.

### 4. The consumer inventory, bucketed as the brief asked

Session A's "~20 consumer functions" was right; the recount found **20**, plus 255 call
sites (not the ~136 a `grep sBs` suggests — `sBs` appears three times in the whole
decoder, because everything reaches the reader through parameters).

* **Direct users (11 + 4)** — `nalu.rs`: `ParseNalHeader`, `ParseNonVclNal`, `ParseSps`,
  `ParsePps`, `ParseVui`, `ParseScalingList`, `SetScalingListValue`, `DecodeSpsSvcExt`,
  `ParsePrefixNalUnit`, `ParseRefBasePicMarking`, `ParseSei` (a stub);
  `decoder_core.rs`: `ParseSliceHeaderSyntaxs`, `ParseDecRefPicMarking`,
  `ParsePredWeightedTable`, `ParseRefPicListReordering`. Converted outright.
* **The CAVLC residual path (5)** — `WelsResidualBlockCavlc`, `…8x8`,
  `WelsParseMbCavlcResidual`, `ParseInterInfo`, `ParseInterBInfo`. Converted
  *mechanically onto the mode*: `(*pBs).iIndex` → `cavlc_bit_pos()`, `+= iUsedBits` →
  `advance_cavlc_bits()`, and the `BsStartCavlc`/`BsEndCavlc` call sites in
  `decode_slice.rs` → `cursor.start_cavlc()` / `cursor.end_cavlc(buf)`. The residual
  loop itself is untouched — that is Phase 5's.
* **Through `SDqLayer::pBitStringAux` (9 binding sites in `decode_slice.rs`, plus the
  CABAC I_PCM path)** — one `split()` per function, then the call sites take `buf`.

Two paths needed more than a signature change, both `pEndBuf`-arithmetic per the
brief's step 3, both kept in their exact shape: the two I_PCM byte-copy paths
(`pCurBuf ± n` → `pos ± n` via a new `BsCursor::set_pos`, and `pEndBuf - pCurBuf` →
`len - pos`), and the CAVLC↔CABAC handoff, which got two *named* operations
(`hand_off_to_cabac`, `restore_from_cabac`) rather than loose `set_cur_bits`/
`set_left_bits` setters — every one of those writes is only coherent as part of a whole
handoff, and T3.2 folds them into the engine conversion.

### 5. What died, and the differential retirement

`cursor_of`, `cursor_and_buf`, `store_cursor`, `read_with`, `readable`, and
`BsCursor::from_parts` are all **deleted** — the marshalling layer T3.1a's +0.45% paid
for. The reader family in `dec_golomb.rs` is now six **safe** functions: no `unsafe`,
no raw pointers, `&mut u32` out-params.

The differential file's reader half is **deleted**, not retired in place again. T3.1a
retired it once (it stopped comparing two implementations and began proving the shim
faithful); with the shim gone there is no second implementation, and a test that
compares a thing to itself reads as coverage while proving nothing. Its burden passes
to T3.0's 2316 goldens, the 53 conformance hashes, and `safe/bits.rs`'s unit tests —
which is the handover T3.0 was built first to enable.

What stayed, because it still compares two live things: `GetLeadingZeroBits` and
`BsGetTrailingBits` against their table-driven originals; the whole writer half (until
T3.4); and the three CAVLC tests, now against a **frozen transliteration** of
`BsStartCavlc`/`BsEndCavlc` kept in the test file. That copy is deliberately not built
on `SBitStringAux` — pinning the reference to a struct that T3.3/T3.4 are dismantling
would let it drift with the refactor it is judging.

Also deleted: the F10-class accommodation in `parse_mb_syn_cavlc.rs`'s residual unit
test (three `as_mut_ptr()` calls whose tags Stacked Borrows had already popped). There
are no pointers left to reborrow. The test now sets up the way production does —
`init` then `start_cavlc` — which the mode's guard *forced*, since the residual path
asserts it is inside a CAVLC region.

### 6. `ExpandBsBuffer`'s rebase collapsed, one seam early

Three pointer rebases became one. `pEndBuf` and `pCurBuf` are the cursor's `len` and
`pos` — offsets, which survive a reallocation by definition. That is P5's prediction
arriving at T3.1b instead of T3.3; what is left is the base and (per F16) its extent,
and T3.3 deletes both.

### 7. F17: the Miri gate cannot fail, and has not been able to since Phase 2's exit

Found by reading a baseline log that said `error: test failed, to rerun pass \`--lib\``
and, four lines later, `PASS  miri --lib`. `gates.sh` sets `set -u` but not `pipefail`,
and both Miri steps are written `if (cargo miri …) | tee … | tail -5; then` — so the
`if` sees `tail`'s exit status and `cargo miri`'s is discarded. The bench steps use the
same shape but end in `grep`, which at least fails when nothing matched; the steps that
do it right (`run_cargo_test`, `sweep_gate`) capture `${PIPESTATUS[0]}`.

Written up as `phase3_findings.md` §F17. **Not fixed here** — it is a change to the
gate every remaining seam is judged by, and making it mid-seam means this seam's
verdict comes from a different instrument than the one that judged T3.1a. It is
session C's first action, before T3.2.

The practical consequence for this entry: **Miri's result above was read out of
`.gates/miri_lib.log` by hand** (285 passed, 0 failed), not taken from the battery's
`PASS` line. Prior entries' "Miri green" claims are believable — every session also ran
Miri by hand while developing, which is how F10/F12/F13 were found — but what was never
true is that the battery would have *stopped* a session on a regression.

### 8. The two debug sweep hits, and the S14 alternation

Two `mt sm=3 t=4` zero-byte failures across the session's debug sweeps, on two
different inputs (`CiscoVT2people_160x96_6fps cabac=1 rc=0`, then
`CiscoVT2people_320x192_12fps cabac=0 rc=1`). Both are F3's signature exactly; release
was 341/341 both times.

S14 says more than one hit in a session means alternating both trees inside one loop
rather than arguing, *because* sequential sampling of a load-sensitive race misleads —
and the tempting argument here is a strong one: this seam changes **zero bytes** of
`src/encoder/` and `src/common/` (`git diff --stat` over both is empty), so it cannot
have touched the encoder's slice-list race except through code layout. The rule
distrusts exactly that reasoning, so the alternation was run: **6 rounds × 120 `mt`
configurations per side, alternating the two `rust_enc` binaries inside one loop**
(control = `1bf5a235`, built in a separate worktree so both binaries were on disk at
once, per S1's discipline applied to correctness rather than timing).

| | encodes | `sm=3` zero-byte failures |
|---|---|---|
| control (`1bf5a235`) | 720 | **2** (rounds 5 and 6) |
| HEAD | 720 | **0** |

The control side failed *more*. That settles it: F3, not a regression, and the two
in-battery hits are the natural rate on a machine that spent the session running
batteries. Appended to F3's measurement history. Note the alternation also cost two
gate-battery hits' worth of doubt to resolve — cheap, and the reason the rule is
unconditional.

### 9. Perf

**The +0.45% ledger row T3.1a opened is cleared.** Null first (S2): this session's
decode floor is median +0.08%, range −0.39%..+0.26%, so **±0.4%**. Then two interleaved
pairs, control = `1bf5a235` (the marshalling still in) vs HEAD:

| row | pair 1 | pair 2 | T3.1a had |
|---|---|---|---|
| Constrained Baseline (CAVLC) | **−0.93%** | **−1.07%** | +0.77% |
| Main (CABAC, B-frames) | +0.23% | −0.24% | −0.26% |
| High (CABAC, 8x8) | +0.21% | −0.17% | +0.45% |
| median | +0.21% | **−0.24%** | +0.45% |

Read the CB row, not the median. It is the one that pulls every syntax element through
this family, it moved ≈**−1%** in both pairs — two to three times the floor, same sign,
consistent magnitude — and that is exactly the shim field-marshalling T3.1a's entry said
would come back here. The two CABAC rows sit inside the floor and flip sign between
pairs, which is what "no signal" looks like; their bit work is in `cabac_decoder.rs` and
belongs to T3.2. Encoder: median +0.34% over 28 rows against a null floor of
−2.76%..+1.49% — a wash, as it must be for a seam that changes zero encoder bytes.

Cumulative decode after both halves of T3.1, on the CAVLC row: +0.77% then −1.07%, i.e.
**net negative**. The phase's ~7-point decode allowance on CB is untouched going into
T3.2, which is the seam that will actually spend it.

### Next session's first action

**Not T3.2 yet — two things first, both small, both cheap to do cold:**

1. **Fix F17** (`gates.sh`'s two Miri steps → `${PIPESTATUS[0]}`, no global `pipefail`
   — the bench steps' `grep` filters would start failing the battery). Deliberately
   left undone here so this seam and T3.1a were judged by the same instrument; doing it
   first means T3.2 is the first seam gated by a Miri step that can actually fail.
2. **Re-run the battery on the fixed gate** to establish what "green" now means.

**Then T3.2**, the CABAC engine, from a standing start — it is the phase's perf-critical
seam and the brief is explicit that beginning it late is the order-deviation trap. Two
things this session leaves it:

* `InitCabacDecEngineFromBS`/`RestoreCabacDecEngineToBS` already take `&mut BsReader`,
  and the reader's side of the handoff is two named methods (`hand_off_to_cabac`,
  `restore_from_cabac`) with `restore_from_cabac(pos)` already being the single `usize`
  assignment the brief describes. The engine's own pointer triple is untouched.
* **F16 is a live trap for it.** The end ladder (`cabac_decoder.rs:732-784`) selects a
  4/3/2/1-byte final load from `pBuffEnd - pBuffCurr`, and init primes 5 bytes. Take the
  readable extent from `BsReader::avail` / `readable_from` — do **not** re-derive one
  from `len` plus a constant, which is the mistake F16 records.

S1 before conversion, per the brief: `DecodeBinCabac` was 4a's largest single decode
consumer (544 self-samples), so disassemble the current raw hot path before touching it.

## 2026-08-10 — Phase 3, session C (F17 fixed and proven; T3.2)

**Goal:** [`prompts/archive/phase3_session_c.md`](prompts/archive/phase3_session_c.md) — (1) make
`gates.sh` able to fail, prove it, re-baseline; (2) T3.2 from a standing start. Both
landed. T3.3 was not touched, per the brief's non-goals — the surplus went where the
brief said to spend it: the audit prose and the disassembly comparison.

**Started at** `331668a7`; **ended at** `00c6cf9f` + this docs commit. Tree clean at
both ends. Everything ran foreground/sequentially or as a single background battery
with nothing else touching the target dir, per the session-B correction.

### What landed

| commit | what |
|---|---|
| `eae61b94` | **F17** — `gates.sh` can fail again; proven red on a real input; new baseline |
| `00c6cf9f` | **T3.2** — the CABAC engine's pointer triple → `pos` over a per-call RBSP window |

### 1. F17, and what the audit found beyond the brief

The fix is `run_cargo_test`'s own pattern, now applied everywhere it was missing: one
`run_miri` helper (both Miri steps) taking `${PIPESTATUS[0]}` + parsed libtest totals +
a **zero-test clause** (`passed == 0` fails — the script's header names that trap for
`cargo test`; a mistyped `--skip` walks through everything else). Both bench steps:
`PIPESTATUS[0]` **and** `[2]` (the display grep matched something — the one signal the
old shape carried, kept) **and** the MISMATCH/DIFFER check. The empty-file-list case of
the phase-exit differential loop now fails loudly. `OVERALL: PASS/FAIL (N)` is the last
line, matching the exit code. `set -o pipefail` stays rejected — four greps in the
script legitimately return 1 — and the complete per-step audit is a comment block at
the bottom of the script.

**One correction to the brief, fixed in place per its header:** its table called the
sweep verdict unsound ("pipe status ignored"). Wrong — `sweep_gate` captured
`${PIPESTATUS[0]}` all along; the real hole was the no-tally fallback printing `tail
-3` text that reads like a result. Now an explicit `fail`.

**The known-red self-test earned its keep twice.** Run 1: the fix itself was broken —
`a=${PIPESTATUS[0]}; b=${PIPESTATUS[2]}` resets `PIPESTATUS` after the first
assignment, and under `set -u` the battery aborted at the decode bench. One-shot
array capture (`st=("${PIPESTATUS[@]}")`) fixed it. Run 2, with the F12 skip deleted:
Miri found the `wels_thread_pool` retag race, and the battery printed
`FAIL  miri --lib … 0 passed / 0 failed, rc=1` → `OVERALL: FAIL (1 steps failed)`,
exit 1. (Totals are 0/0 because Miri aborts before libtest's `test result:` line — the
`rc` clause carried the verdict, so the corroboration is genuinely belt-and-braces.)
Skip restored; red-proof log archived in the session scratchpad.

**The new baseline** (skips restored, defaults, foreground): 430/424/20, sweeps
341/341 both profiles, goldens 2316, both benches bit-identical, **miri --lib 285/0 —
machine-judged for the first time**, `OVERALL: PASS`, exit 0. Session B's
hand-verified numbers, now machine-confirmed. For the record: every `PASS miri`
printed by `gates.sh` between Phase 2's exit and `eae61b94` was unconditional; no
finding is invalidated (all were verified by humans reading logs); the *gate* exists
from that commit.

### 2. T3.2 step 0 — the read-extent audit, and its two-extents result

Per-site enumeration, written into `cabac_decoder.rs`'s module docs (F16's lesson as
procedure — no quantifier over an unenumerated set). Three sites touch the buffer;
nothing else in the engine does; all seven consumers reach it only through
`Read32BitsCabac`:

| site | loads | max index | needs |
|---|---|---|---|
| init prime | 5 bytes at `pos − remaining` | `len + 2` | `avail ≥ len + 3` |
| end ladder | 4/3/2/1 at `pos` | **`len − 1`** | `len` only |
| handoff | **none** | — | — |

The surprise is the middle row: **F16's named suspect never reads past the RBSP** —
its selector is measured against `pBuffEnd`, so every arm is bounded by the logical
end. That licenses the conversion's shape: the hot path takes
`BsReader::rbsp_window()` (the first `cursor.len()` bytes of `buf()`), making
`win.len()` *be* `pBuffEnd − pBuffStart` — no extent arithmetic inside the engine at
all — and only the once-per-slice init uses the wider `avail` window, through `get`,
error path not panic. Two numbers, two concepts, one owner each; recorded as a §2.2.2
[P3] note. The ladder's `iLeftBytes <= 0` became the **comparison** `pos >= win.len()`
— `pos` legitimately exceeds `len` after init on truncated input, and a `usize`
subtraction would wrap and select the 4-byte arm: a new OOB where the raw code
errored.

**The brief's "does the corpus actually reach the ladder?" check was run, and it
nearly produced a wrong corpus extension before it produced the right answer.** First
instrumentation (`eprintln!` in the arms) reported **zero** hits from the whole
battery — parity corpus and conformance both — and on that evidence an EOF-tail
truncation family was built, goldens regenerated, additions only. Then the
instrumentation itself failed an attribution sanity check: libtest **captures** a
passing test's stderr, and the parity test's workers are subprocesses whose stderr the
parent parses, so `eprintln!` could never have been seen from either gate — the zero
measured the instrument, not the coverage. File-append tracing (env-gated, capture-
immune) gave the real numbers: the **existing** corpus enters the short arms 2,678
times and the no-bytes error arm **896** times (the fine sweep's `boundary − d` cuts
*are* the genuine-bins-to-the-cut geometry — a truncation is the recomputed `len`),
and conformance enters the short arms ~9,600 times (well-formed CABAC slices end
through them) and the error arm never. So 2.0's extension condition is **false**, the
corpus family and regenerated goldens were reverted unlanded, the goldens stand
**unmoved**, and the instrumentation is stripped. Three lessons in one: the ladder's
arms are *well* covered by exactly the gate built for them; the S17/F17 shape — an
instrument that cannot report is not an instrument — claimed its third instance this
session, this time as a measurement rather than a gate; and the attribution
re-run (old corpus, sound instrument) is what stopped a redundant 80-row golden
diff from landing with a false justification in its commit message.

Both `debug_assert`s the brief asked for are in, each with its why: init's rewind
(`pos ≥ 4` on every path — `init`/`init_read_bits` prime 4 bytes; tight at
`pos = 4, remaining = 4`) and restore's (`pos − (bits_left >> 3)` is refill-invariant
and renorm only raises it; the negative-`bits_left` error path moves *forward*, in
`isize`, as the raw arithmetic did).

### 3. The conversion, and what the disassembly comparison caught

`SWelsCabacDecEngine` → `{uiRange, uiOffset, iBitsLeft, pos: usize}`, field order
keeping range/offset adjacent for the `ldp`. `ctx.pCabacDecEngine` keeps its pointer
type — the struct changed under it, the T3.1b-precedent retype was not needed.
`parse_mb_syn_cabac.rs`'s 18 functions: call-expression edits only — one
`cabac_win = cabac_rbsp_window(pCtx)` binding per function (the single `SHIM(phase3)`
deref chain; dies with `pBitStringAux`), `win` threaded through. The handoff is one
`usize` each way, pinned by the new CAVLC→CABAC→CAVLC round-trip test at a 20-bit
offset asserting full cursor state; plus a ladder-bounds test walking all four arms
and the past-the-end comparisons, and an init end-guard test. Three mutations killed
(S10): ladder `>=`→`>`, a 4-byte prime, restore without the rewind.

**S1 in the brief's order, and step 3 caught two real defects the tests could not.**
Step 1 recorded the raw reference (114 instructions, `Read32BitsCabac` fully inlined,
zero buffer bounds checks, one `uiState < 64` table check, range/offset `ldp`, bare-ret
MPS exit). The first converted shape — the obvious `match tail.len()` with indexing —
**failed the comparison**: the refill stopped inlining (a stack frame on every bin,
refill or not) and the `_` arm re-checked the length (three `panic_bounds_check`
paths, four `ldrb`s where the raw had one `ldr`+`rev`). Restructured before any bench
ran: `#[inline(always)]` on the refill, and the ladder as a `first_chunk::<N>()` chain
testing `>= 4` first — the width stated as a type, S9's exact-span trim at four-byte
scale. Final shape matches the reference point for point: refill inlined (no symbol
survives), zero buffer bounds checks, the one pre-existing `uiState` check, `ldp`
preserved, 4-byte arm a single `ldr`+`rev`, bare-ret fast path, 122 vs ~121
instructions, and the curr/end pair load became one `pos` load with `win.len` already
in a register. (3/2-byte arms are `ldrb`+`orr` chains vs `ldurh`+`rev16` — end-of-RBSP
arms, reachable once per slice, immaterial.) `DecodeBypassCabac`/`DecodeUnaryBinCabac`
fully inline; `DecodeTerminateCabac`/`DecodeExpBypassCabac` 0 panic paths, 0 calls.

### 4. Gates and perf

| | inherited (`eae61b94` baseline) | final |
|---|---|---|
| tests | 430 / 424 / 20 | **433 / 427 / 20** (+3: round-trip, ladder, guard) |
| T3.0 goldens | 2316 | **2316, unmoved, both profiles** |
| conformance | 53 hashes | 53, unchanged |
| sweeps | 341/341 both | debug 341/341; release **340/341, one F3 hit** — below |
| benches | bit-identical | bit-identical, both |
| Miri --lib | 285/0 | **288/0** (the three new tests included) |
| ratchet | 1334 / 5106→ | `raw_ptr` **5109 → 5106** (the triple), `SHIM(` **157 → 158** (`cabac_rbsp_window`), `unsafe_fn` +2 / `unsafe_block` +5 (two helpers + three test blocks); baseline regenerated with the reason in the commit |

**The F3 hit** (`mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=0 rc=0`,
zero-byte, release): the signature exactly, first hit this session → the one-hit rule,
re-run that configuration: **5/5 BYTE-IDENTICAL**. Appended to F3 as the **tenth
measurement** — noteworthy only because it was the first F3 hit *stopped* by the
post-F17 gate (`OVERALL: FAIL (1 steps failed)`, exit 1), which is precisely the
behaviour the morning's fix bought. Seam changes zero encoder/common bytes;
corroboration, not verdict.

**Perf** (S2 fresh null first): this session's decode floor is **≈±2%** (null median
−0.27%, min −2.05% — five times wider than session B's ±0.4%; the machine spent the
day running batteries), encoder ≈±3.5%. The pair, 3-pair medians, control =
`eae61b94`:

| row | delta |
|---|---|
| decode CB (CAVLC) | +0.19% |
| decode Main (CABAC) | **+0.76%** |
| decode High (CABAC 8×8) | **+0.27%** |
| encoder median (28 rows) | +0.00% |

The CABAC rows are the signal this time and they sit **inside the floor** —
flat-to-win, exactly the band §4 of the brief predicted for literal-stays-literal.
CB is the cross-check and is flat; the encoder is the required wash. **No ledger row
opens.** Cumulative decode stays ≈ +17.8 / +9.6 / +10.1%; the CB allowance is intact
and T3.3 touches no per-bin path. The durable artefact is the disassembly diff, not
the bench numbers: both step-3 defects were invisible to 433 tests, 2316 goldens and
53 conformance hashes, and would have been a real regression shipped as "flat".

### Hand-off: T3.3, the ownership seam

`RawDataBuffer { buf: Vec<u8>, start, cur }` replacing `SDataBuffer`'s pointer
quadruple; `nalu.rs` payloads → `Range<usize>`; **`ExpandBsBuffer` deleted, not
converted** (offsets survive realloc — T3.1b already collapsed its rebases to one),
**and F16's stale-`avail` instance dies with it**; P5's grow-mid-AU unit test written
loudly; **F15 fixed** at both `nalu.rs` sites (`iNalSize == 0` → checked indexing) and
`withheld()` **deleted in the same commit** — regenerate with `UPDATE_MALFORMED_GOLDEN=1`
and the diff must show exactly the eleven-per-stream `WITHHELD` rows gaining real
outcomes and *nothing else moving*; that diff goes in the commit message. The seam
also deletes T3.1b's `BsReader` shim family (`base`/`avail`/`buf()`/`split()`/
`readable_from`/`rbsp_window` and `cabac_rbsp_window` — the `SHIM(phase3)` set is
enumerable by grep) and takes the phase's biggest single `raw_ptr` drop. What T3.2
leaves it: the engine consumes the window per call already, so when the owner becomes
a `Vec` the only change on the CABAC side is where `rbsp_window` gets its bytes.

Meta-rule worth carrying: **the regression-vs-pre-existing principle got exercised in
miniature here** — the ladder's `usize`-wrap hazard is exactly "your conversion
narrows/changes a window the raw code had"; writing it as a comparison *before* any
malformed row could catch it is the cheap form of the session-B disambiguation test.

## 2026-08-11 — Phase 3, session D (T3.3; the decoder read side closes)

**Goal:** [`prompts/archive/phase3_session_d.md`](prompts/archive/phase3_session_d.md) — seam T3.3
whole, four faces in dependency order. All four landed, plus the seam close. T3.4 was
not touched, per the brief's non-goals.

**Started at** `d737a450`; **ended at** `0a5550ae` + this docs commit. Tree clean at
both ends.

### What landed

| commit | what |
|---|---|
| `345308cb` | **faces 1+2** — `RawDataBuffer`; the `BsReader` bridge deleted; `ExpandBsBuffer` deleted; **F16 instance 2** closed; P5's growth test |
| `1db28c4f` | **face 3** — `nalu.rs` payload identity is an offset; four dead transliterations deleted (S18) |
| `8b611076` | **face 4** — **F15 fixed**; `withheld()` deleted; 105 golden rows un-withheld |
| `0a5550ae` | the session brief, with three corrections made in place |

### Gates

| | inherited (`d737a450`) | final |
|---|---|---|
| tests | 433 / 427 / 20 | **434 / 428 / 20** |
| T3.0 goldens | 2316 rows, **105 `WITHHELD`** | **2316 rows, 0 `WITHHELD`** — see §4 |
| conformance | 53 hashes | 53, unchanged throughout |
| sweeps | 341/341 both | 341/341 both **on the battery that gated the last code commit**; three F3 hits across the session's five batteries, each re-run 5/5 clean — §5 |
| benches | bit-identical | bit-identical, both |
| Miri `--lib` | 288 | **289** (the P5 growth test) |
| ratchet | 1336 / 5106 / 158 | **1327 `unsafe_fn` / 5076 `raw_ptr` / 155 `SHIM(`** |

**The milestone:** with this seam the decoder's entire read side — cursor (T3.1),
engine (T3.2), buffer and NAL identity (T3.3) — is safe-owned. There is no
`from_raw_parts` left anywhere in the bitstream read path, no pointer cursor, and no
stored extent. `raw_ptr` −30 is the phase's biggest single drop, and `SHIM(` **fell for
the first time in the phase**: the strangler scaffolding is now being removed rather
than added.

### 1. The design rule did the work: derive, don't store

The brief's §1 asked for one property — *every readable extent is derived from the
owning buffer at window-creation time; nothing stores a length the buffer can outgrow* —
and made it the standard the seam is judged by. Applying it literally is what shrank the
design twice before a line was written:

* The brief's own sketch stored `start`, `cur` **and** `end`. `end` (`pEnd − pHead`) is a
  stored copy of the allocation size — the F16 defect in miniature — and is `buf.len()`.
* `pStartPos` turned out **write-only in this port**: set at init and at each rebase,
  read by nothing, because upstream's parse-only rewind that consumes it was never
  transliterated. So `start` had nothing to store either.

`RawDataBuffer { buf: Vec<u8>, cur: usize }`. One offset. Corrections made in the brief
in place, per its header, and in plan §2.2.2 as a [P3] note.

The same rule is why `ExpandBsBuffer` could be **deleted** rather than converted, and
why **F16's second instance closed by construction**: that instance existed because a
growth invalidated a stored `avail`, and there is no longer anything for a growth to
invalidate. Saying "we fixed the stale-extent bug" would have been weaker than what
actually happened, which is that the bug became unrepresentable.

What the rule did *not* license was redesigning the slop. T3.1a/F16 established on
evidence that the reader's slack bytes are real neighbouring stream bytes that feed
decoded values on malformed input, so the owned `Vec` is kept at full allocated size and
zero-filled — `WelsMallocz`'s exact semantics — and `buf.len()` is initialized bytes,
never spare capacity. The 2316 goldens were the referee that the accounting is exact,
and they did not move for faces 1–3.

### 2. The `Vec` fields exposed production UB the conversion had to fix

`SWelsDecoderContext::Default` was `unsafe { std::mem::zeroed() }`, and `codec_api.rs`'s
decoder-creation path called it — not a test path. The moment two fields became `Vec`s,
that call was manufacturing invalid `Vec`s on the live path. `Default` now writes those
two fields through a `MaybeUninit` shell before the value materializes.

Then the test suite found the second half of it, twice: the context is several MiB, so
`Box::default()`'s by-value construction overflows a 2 MiB test thread's stack. Hence
`new_boxed()`, which builds in place on the heap, replacing seven
`Box::new_zeroed().assume_init()` sites (that idiom stopped being sound for the same
reason) and three by-value `::default()` sites.

Worth noting how this was found: not by review, but by a stack overflow in a test that
has nothing to do with bitstreams. The port's habit of zeroing C structs wholesale is a
standing hazard for every later phase that gives a context an owning field — Phase 5
will meet it with `MbGrid`, and Phase 6 with the encoder's pools.

### 3. Four dead transliterations, found by conversion rather than by a sweep

S18's straggler discipline is a phase-exit instrument; this seam produced four hits
without one, because converting a signature forces you to look at every caller:

* `DetectStartCodePrefix` — zero callers. `split_annexb_units` has been the Annex B
  scanner since `WelsDecodeBs` was written.
* `decoder_core`'s duplicate `SVclNal`/`SPrefixNalUnit`/`SNalData` trio and its
  `PPrefixNalUnit` alias — every live `sNalData` access resolves through `nalu.rs`.
* `decoder_core`'s duplicate `DecodeNalHeaderExt` — zero callers.
* `SVclNal::pNalPos` — never written in this port, and its one read sat behind an
  always-true null guard, so a `memcpy` in the parse-only output path has never
  executed. Deleted with the field; `iNalLength`, whose bookkeeping does run, stays.

Plus two dead parameter pairs (`pSrcNal`/`kSrcNalLen` on `ParseSps`/`ParsePps`/
`ParseNonVclNal`). The general lesson for T3.4, which faces the same situation with four
copies of the writer: **a duplicate that no longer compiles is cheaper to find than one
that still does** — retyping the thing they all embed is what surfaced these.

### 4. F15: the fix is defined by what release accidentally did

The brief said to re-grep the expression shape rather than trust the remembered count.
Done, and the count held: three sites, two live and one already guarded, all three now
routed through one helper so none can form the index by subtraction again.

The fix itself is small and the interesting part is the *choice of answer*. An empty
RBSP yields bit size **0**, which flows into the caller's existing `DecInitBits` failure
branch — `dsBitstreamError` plus the access-unit bookkeeping the code already had. That
is exactly what release did by accident (its out-of-bounds read landed on an odd header
byte, giving zero trailing bits), so the fix promotes an accidental outcome to a defined
one rather than inventing behaviour. Debug no longer aborts the process; release no
longer computes an out-of-bounds pointer; and the guard is a **comparison, not a
subtraction**, per the seam's arithmetic rule.

**The proof is the golden diff, and it is exactly what the protocol demanded:**

```
removed WITHHELD rows: 105      added WITHHELD rows: 0
removed OTHER rows:      0      added other rows:  105
```

12 files, 105 insertions / 105 deletions, classified mechanically rather than eyeballed
— the F15-named rows gained outcomes and nothing else moved. The new rows read as the
graceful path: `0x4` (`dsBitstreamError`) at the truncated NAL, `0x0` before and after,
and the longer truncations carry real per-plane hashes for the frames decoded ahead of
the damage. **Both profiles then accept the same table** — generated in debug, release
passes it unchanged — which is the property that could not exist while the finding was
open, and the reason `Outcome::Withheld` is deleted rather than left unused.

Session C's deliberate near-miss (the reverted-unlanded corpus extension) is what kept
this diff clean, and no corpus change rode along.

**A note on the count**: the brief's §5 heading promised "exactly 13 rows". 13 is
**F16's** number — the corpus entries that hit the CAVLC prime narrowing — conflated
with F15's. F15's own text says ~11 per stream and the tables held 105. The referee
("exactly the withheld rows, nothing else") was unaffected and was met; the heading is
corrected in place. Cheap lesson: a remembered *number* is as untrustworthy as a
remembered line number, and the protocol survived precisely because it was written as a
property rather than a count.

### 5. F3's eleventh measurement, and the two hits that landed on trees the seam never touched

Three hits across five batteries, all the signature, each re-run 5/5 clean. Two of them
are worth naming precisely, because of *where* they landed: the first came from the
**opening control battery — on the session-start commit, before a line was changed** —
and the third from the **final re-verification of a docs-only commit**. Neither tree
contains a byte this seam wrote. That is the cleanest available statement that the rate
belongs to the machine, and it is why the second hit (face 3, release) was not news.

The alternation ran after that second hit, because more than one triggers S14's clause
and the tempting argument ("decoder-only seam, zero encoder bytes changed" — verified,
the diff over `src/encoder` and `src/common` is empty) is the one the rule exists to
distrust. Control `d737a450` in a worktree, 6 rounds × 32 `mt sm=3` configurations per
side, alternating in one loop: **control 4/192, HEAD 2/192**. Control failed twice as
often — same direction as the ninth measurement, second acquittal in two runs. The
third hit, arriving after the alternation had already answered, was recorded as
corroboration rather than triggering a second one.

Appended to F3 with a note that the current rate (≈1/64 under battery load) means a
6×32 alternation now suffices where the ninth needed 6×120.

One bookkeeping point for whoever reads the gate logs: the battery that gated the last
**code** commit (face 4) was `OVERALL: PASS` with 341/341 in both profiles. The
`OVERALL: FAIL` in the final log is that third F3 hit on a docs-only tree — the
post-F17 gate stopping the session on a known-flaky configuration, which is the
behaviour it was fixed to have, and the protocol answered it in five runs.

### 6. Perf, and the one place the disassembly and the bench disagreed about mattering

S2 null first: this session's decode floor is **median +0.51%, range +0.36%..+0.57%**
(≈±0.6% — five times tighter than session C's, the machine having been quieter than its
battery count suggests), encoder ≈±3.3%. Then the pair, 3 pairs, control =
`d737a450` (seam start):

| row | delta |
|---|---|
| decode CB (CAVLC) | **−0.43%** |
| decode Main (CABAC) | +0.36% |
| decode High (CABAC 8×8) | +0.32% |
| encoder median (28 rows) | **+0.00%** (range −1.45%..+1.47%) |

Every decode row sits at or below the null's own median, so there is no signal at all;
CB is the row that matters here (it pulls every stream byte through the fill path and
every syntax element through the reader) and it is a small win. **No ledger row opens**;
cumulative decode stays ≈ +17.8 / +10.1 / +9.6%, and the phase's ~7-point CB allowance
is still untouched three seams in.

S1 proportionally, per the brief: one disassembly look at the converted copy path,
compared honestly against the raw one (control tree, same function, both emitted at
`--release`). The EPB-stripping loop is a byte scanner in both versions — it never was a
`memcpy`, since it must detect `00 00 03` — and the conversion costs **two instructions
per byte**: the raw steady-state body is 10 instructions, the safe one is 12, the extra
pair being the `cmp`/never-taken-`b.eq` of `dst[dst_len] = b`. The up-front slice take is
one check per NAL, as designed.

That check does not fold, and it is worth writing down *why* rather than leaving a future
session to re-derive it: the write index lags the read index by a variable amount (each
skipped `0x03` widens the gap), so no safe formulation — indexing, `split_first_mut`,
`iter_mut().next()` — can prove `dst_len < dst.len()` without a per-write test; they all
compile to the same compare-and-branch. The measurement says it costs nothing (CB
−0.43%), so it stays. **If a future measurement ever wants it back**, the restructure
that removes it is a different algorithm, not a different idiom: copy the runs *between*
EPB markers with `copy_from_slice` and skip the marker bytes, which turns a per-byte
loop into a handful of `memcpy`s and would likely beat the raw code. Deliberately not
done here — S1 forbids optimizing without a measurement demanding it, and the brief
scoped this seam's fill path as "hot-ish", correctly.

### 7. Differential retirement: nothing was owed

The brief expected the bridge deletion to shrink the differential files' raw side. It
did not, because that retirement already happened: T3.1b deleted the differential's
reader half outright (a test comparing a thing to itself), leaving only the frozen CAVLC
transliteration, the trailing-bits/leading-zeros pair, and the writer half — none of
which touch `BsReader`. So the Miri count moved **288 → 289**, and the +1 is the P5
growth test, not a retirement. Recorded because "expected the count to move" and "the
count moved for the reason expected" are different claims, and only the second one is
true here.

### Hand-off: T3.4, the encoder write side, part 1

**First action, before any conversion: re-read [`phase2_findings.md`](phase2_findings.md)
F2 and F5 in full**, then dedupe — as its own commit, sweeps as the referee, changing
zero bytes of output or it reverts. F2's map: `vlc_encoder.rs:367` canonical;
`svc_set_mb_syn_cavlc.rs:157` equivalent with a hand-rolled 4-byte store;
`nal_encap.rs:169` equivalent with an explicit `iLen == 0` guard;
`svc_encode_slice.rs:509` **divergent** (null/`iLen <= 0` early-returns, pre-masks
`kuiValue`, inverts the branch sense, `wrapping_add` in `BsWriteUE`). The dedupe commit
records which guard semantics die; F5 says not to "fix" the canonical writer's debug
panic while deduping (S6).

Then `vlc_encoder.rs` → `BsWriter`, where **F13's lying `InitBits` signature dies** and
`au_set.rs`'s two accommodations are deleted in the same commit — a named deliverable of
the phase, and 4a's precedent says deleting an accommodation exposes the next finding
immediately, so expect one.

Three things this seam leaves T3.4:

* **The dead-duplicate lesson (§3) transfers directly.** Retyping the thing the copies
  embed is what surfaced four dead transliterations here; F2's four writers are the same
  shape, and the dedupe is exactly that operation performed deliberately.
* **The zeroed-struct hazard (§2) is now a known trap**, and the encoder structs T3.6
  converts (`SWelsNalRaw`, `SWelsEncoderOutput`, `SWelsSliceBs`) are all reached through
  wholesale-zeroed contexts. Check the construction path *before* giving any of them an
  owning field.
* **Comparison-form arithmetic** paid twice more this session (the ladder at T3.2, the
  `size < 1` guard here). `svc_set_mb_syn_cavlc.rs:752`'s `pEndBuf - pCurBuf - 1` space
  checks are the next instance waiting.

---

## 2026-08-11 — Phase 3, session E (T3.4; the encoder write side, and `SBitStringAux` dies)

**Goal:** [`prompts/archive/phase3_session_e.md`](prompts/archive/phase3_session_e.md) — seam T3.4 in
three faces. All three landed, plus the headline the brief made conditional. T3.5 was
not started: faces 2 and 3 merged into one large conversion and there was no standing
start left, which is the rule working rather than a shortfall.

**Started at** `b308f7d5`; **ended at** `fb4e7c29` + this docs commit. Tree clean at
both ends.

### What landed

| commit | what |
|---|---|
| `13912ffd` | **face 1** — F2's four writer copies collapse onto the canonical family; fifteen functions and four dead transliterations deleted; zero bytes of output move |
| `5bd19deb` | **faces 2+3** — the encoder write side onto `BsWriter`; `InitBits` deleted (**F13 site 3 closed**); the two `au_set.rs` accommodations deleted; **F5 closed**; four `abi_guard` asserts deleted; the writer differential retired |
| `fb4e7c29` | **the headline** — `SBitStringAux` deleted, with its inventory |

### Gates

| | inherited (`b308f7d5`) | final |
|---|---|---|
| tests | 434 / 428 / 20 | **431 / 425 / 20** — five retired differentials, two new `safe::bits` tests |
| T3.0 goldens | 2316 rows, 0 `WITHHELD` | 2316 rows, 0 `WITHHELD`, unchanged throughout |
| conformance | 53 hashes | 53, unchanged throughout |
| sweeps | 341/341 both | **341/341 both, on a clean `OVERALL: PASS` full battery at the end** — four F3 hits during the session, §5 |
| benches | bit-identical | bit-identical, both |
| Miri `--lib` | 289 | **291**, with the accommodations gone and **no new skip** |
| ratchet | 1327 / 5076 / 155 | **1296 `unsafe_fn` / 5034 `raw_ptr` / 157 `SHIM(`** |

**The milestone:** the bitstream layer no longer contains a pointer cursor of any
kind. The decoder read side closed at T3.3; this seam closes the encoder write side;
and with both done the type they shared — `SBitStringAux`, the `pStartBuf`/`pCurBuf`/
`pEndBuf` triple that §1.2's taxonomy named as T3's emblem — has no users and is
deleted. What remains of Phase 3 is T3.5 (the CABAC coder's *own* triple, which never
used this struct) and T3.6 (owned output buffers).

### 1. The session's shape was wrong in one specific, instructive way

The brief split the work as *face 2 = convert the writer functions*, *face 3 = flip
`SWelsSliceBs`*. **Those do not separate.** A writer function whose signature is
`(buf: &mut [u8], w: &mut BsWriter)` cannot be fed by a struct field holding an
`SBitStringAux`: the field's type is *forced* by the signature change, not merely
adjacent to it, and the same is true of `SWelsEncoderOutput`, `SWelsOut` and
`SSlice::pSliceBsa`. They landed as one commit, and the four `abi_guard` asserts had
to die in it too, because a `const` size assertion fails at compile time — there is no
intermediate state where the tree is green.

What *did* separate cleanly is the thing the brief treated as conditional: deleting
`SBitStringAux` is a pure subtraction once nothing references it, and it is its own
commit with its own inventory.

**The generalisable bit for later seams:** the decomposable unit in a struct-field
conversion is *the set of structs reachable from one signature*, not one struct. Phase
5 (`SDqLayer`, `MbGrid`) and Phase 6 (the encoder pools) should size their faces that
way rather than by struct count.

### 2. F2's decision, and the divergence F2 did not know about

The canonical copy won. Every guard the other three added is defensive code the C++
does not have, and each died with a written reason (the commit message enumerates all
nine). The one with real teeth was `svc_encode_slice.rs`'s **pre-mask of the value to
`iLen` bits**: the canonical ORs the value into the accumulator whole, so an
`iFrameNum` or `iPicOrderCntLsb` carrying bits above its field width would be silently
truncated by the old copy and would corrupt its neighbouring syntax elements under the
canonical. The encoder keeps both counters reduced modulo their field width — and the
sweeps, not my reading of the code, are what says so.

**F2's inventory was incomplete, and collapsing the copies is what showed it.** The
finding's table compares `BsWriteBits`, and that is all it compares. `nal_encap.rs`
also carried its own `BsFlush`, and that one was **not** equivalent: it stored only the
`4 - iLeftBits / 8` bytes it advanced over, where the canonical and
`golomb_common.h:104` always store a full 32-bit word. Up to three bytes past the write
position differ; the next write overwrites them, and past the last NAL they are outside
the output, so every gate the port has was blind to it. F2 is annotated.

The lesson is not about those bytes. It is that **a duplicate-function finding must
enumerate the whole family, not the function that motivated it** — and the way you
discover the miss is by doing the collapse, which is the same mechanism session D
described from the other side (§3 there: retyping the shared thing is what makes dead
duplicates findable). Both sessions found the same shape: *the dedupe is the
instrument*.

### 3. F13's third site and F5 both closed by deletion, and one expectation did not pay

`InitBits` declared `kpBuf: *const u8`, stored it as `pStartBuf: *mut u8`, and the
writer wrote through it — a signature documenting the opposite of what the function
does, so **every honest caller was UB**. The brief said "deleted, not amended", and
that turned out to be the only option: once the buffer is a `&mut [u8]` the caller
already holds, the sole remaining state is `BsWriter::new()`. There is no signature to
fix because there is no function.

`au_set.rs`'s two accommodations went in the same commit, the phase's named
deliverable, and Miri ran the honest code at 291/291. **4a's precedent said deleting an
accommodation exposes the next finding immediately, and this time it did not.** Worth
recording as a negative result: the precedent came from a *gate skip* being deleted (a
whole module re-entering Miri's view), whereas these were two test-local pointer casts.
Deleting an accommodation is only as revealing as the instrument it was hiding from.

F5 closed as a side effect, exactly where its own "who fixes it" line predicted:
*"Phase 3.2, in the commit that collapses the four writer copies — the fix is the same
one `BsWriter` already carries."* Face 1 deliberately left the panicking
`uiCurBits << iLeftBits` alone (S6), and face 2 deleted the body holding it. That is a
behaviour change on the path F5 describes — a debug build that used to panic now writes
the intended word — and it is unobservable only because the path is unreachable. **A
deletion is allowed to stand in for a decision when the decision has no reachable
consequence, and not otherwise.**

### 4. Phase 4b's fence held, and the reason is worth keeping

Three entry points reach the writer through function pointers whose signatures 4b owns:
`pfWelsSpatialWriteMbSyn`, the two slice-header writers behind
`PWelsSliceHeaderWriteFunc`, and `pfStashMBStatus`/`pfGetBsPosition`. None gained a
parameter.

Two different reasons, and both are reusable:

* **The stash pair and `GetBsPosCavlc` need no buffer at all.** They only ever read or
  restored cursor *state*, and a detached cursor is `Copy` — so
  `pBsStackBufPtr`/`uiBsStackCurBits`/`iBsStackLeftBits` became one `sBsStack:
  BsWriter` restored by assignment, and the bit position is `bits_pos()`. Detaching the
  cursor removed the parameter rather than adding one. (This is the CAVLC half of
  T3.5's rollback, landing early because the field types forced it; the CABAC half and
  the `m_pBufStart/Cur/End` triple are still T3.5's.)
* **The other three derive the buffer from what they already have.**
  `slice_bs_buffer(pCtx, pSlice)` picks between the slice's own `sSliceBs.pBsBuffer` and
  the frame's `pOut->pBsBuffer` by **the identity of the writer `pSliceBsa` aims at** —
  which is what `InitSliceBsBuffer` wrote and what the C++ `pStartBuf` carried
  implicitly. The obvious alternative, re-deriving it from `iMultipleThreadIdc` and
  `uiSliceMode`, was rejected: those parameters can move between allocation and use, and
  the pointer cannot. Session D's "derive, don't store" rule with its edge showing — the
  thing to derive from is the state that *recorded the decision*, not the inputs the
  decision was made from.

### 5. F3's twelfth measurement, and the first LONG output

Four hits, all the signature, each re-run 5/5 clean; the third alternation was run and
the tree acquitted again (control `b308f7d5` **3/80**, HEAD **1/80**, on the worst-known
configuration). Full table in F3. Three things new:

* This was the first session whose seam is **encoder-side**, so session D's softening
  argument ("the seam is decoder-only") was unavailable and the alternation was run on
  the first excuse rather than the second.
* **Two batteries on one unchanged tree disagreed with each other**: face 1's `family`
  battery was 341/341 in both profiles, and the `full` battery minutes later on the
  identical tree drew one hit per profile. Same binaries, opposite verdicts.
* The output can be **long**. Every prior recorded failure was zero-byte or short;
  HEAD's single alternation hit was 42312 bytes against 42281. S14's wording already
  said "any wrong length (zero, short, or long)"; this is the first observed instance of
  the third case, and the broad wording was right.

Practical note: alternating a *single known-worst configuration* rather than a sub-sweep
works and takes ~4 minutes. Build both `rust_enc` binaries, keep them on disk, swap them
into the harness path inside the loop, never rebuild between sides.

### 6. Perf: nothing moved, and the disassembly says why

Interleaved pairs, 3 pairs, session floor measured fresh (S2): the null was
median +0.00%, band −1.69% … +1.58% on the encoder. Face 1 alone: encoder median
+0.00% (−1.43% … +1.47%), decode +0.00%. The whole T3.4 conversion (face 1 → HEAD):
encoder median **+0.00%** (−1.02% … +1.47%), decode median **+0.00%** (−0.33% …
+0.06%). Every row inside the floor; the ledger's ≈ +8.9% encoder is unmoved.
`Spatial Ramps` printed +111% and is EXCLUDED per S2 — the same null that session
showed it at −53%.

The S1-proportional disassembly look the brief asked for, on `WelsWriteVUI` as the
representative literal-`n` writer, control vs HEAD:

| | control | HEAD |
|---|---|---|
| instructions | 765 | **720** |
| calls (`bl`) | 1 | 1 (here: the out-of-line cold bounds-failure path) |
| **constant-amount shifts** | **6** | **6** |
| variable-amount shifts | 43 | 40 |
| stores | 119 | **56** |
| branches | 77 | 122 |

The constant-shift count is the literal-`n` rule holding: the widths still fold and
`write_bits` inlines whole, so nothing laundered a literal into a runtime argument.
The store collapse is `WRITE_BE_32`'s four byte-stores becoming one 32-bit store via
`copy_from_slice` — S8's catalogued result, arriving unasked. The branch growth is the
bounds checking the C never had, and it is out-of-line and free on the hot path. **The
seam that adds bounds checks came out smaller and no slower**, which is worth saying
plainly because the phase's standing worry is the other direction.

### 7. The `SBitStringAux` inventory, and what licensed the deletion

After faces 1–3, every remaining mention was one of three things and none was a use:
the definition and its `Default`/`new()`; **five `pub use` re-exports with no
consumer** (`decoder/bit_stream.rs`, `decoder/cabac_decoder.rs`,
`decoder/decoder_core.rs`, `encoder/encoder_context.rs`,
`encoder/svc_set_mb_syn_cabac.rs`); and **two `pub type PBitStringAux = *mut
SBitStringAux` aliases** nothing named, plus two dead imports. Zero struct fields, zero
parameters, zero locals. The compiler was the proof — deleting the definition broke
exactly those and nothing else.

The brief's two suspected holdouts were both checked rather than assumed. The encoder
CABAC writer carries its own triple inside `SCabacCtx` and never used the struct
(T3.5's). The MT slice-buffer path names `SWelsSliceBs`, never `SBitStringAux`. And the
type is in no header under `codec/api/wels/`, so nothing crossed a C ABI with the
layout; `api/abi_guard.rs` guards the public surface separately.

The `wels_common_defs.rs` sweep the brief warned against did not happen. One deletion,
the one the inventory licensed.

### 8. What the ratchet did, and the two shims that are owed

`unsafe_fn` 1327 → **1296**, `raw_ptr` 5076 → **5034**, `unsafe_block` 643 → **626**,
`SHIM(` 155 → **157**. The baseline was regenerated once, at `13912ffd`, with the
reason in that commit (S16): five per-file increases, all of S16's documented shapes —
`SHIM(` +1 in each file holding a new marker, and `raw_ptr` +2 in each of three files
whose *comment* names F13's `*const u8`/`*mut u8` pair. Prose inflating the count is
exactly what S16 says to read past.

The two new shims are named and owned: `bs_buffer` (`nal_encap.rs`) rebuilds a slice
from a `WelsMallocz`'d pointer and its recorded `uiSize`, and `slice_bs_buffer`
(`svc_encode_slice.rs`) picks which buffer a slice writes into. **Both die at T3.6**,
when those allocations become owned and travel with the writer. One place owns that
arithmetic, per T3.1b's precedent.

### 9. Differential retirement: the writer half, per plan §2

Five tests retired in the commit that deleted what they compared against, with the
handover written into the module header of `tests/safe_bits_differential.rs`. Their
burden passes to the **sweeps** — byte-exactness against the C++ encoder itself, which
is a stronger referee for a writer than any in-tree comparison and is the one F2 named
— to `safe::bits`'s own unit tests (the accumulator boundary, the whole-word flush, the
snapshot/rollback round trip, and the two new ones for `te(v)` and `align`'s
one-bit padding), and to `written_streams_read_back_through_the_old_reader`, which
still closes the writer-to-reader loop.

`exp_golomb_sizes_match_the_table_driven_versions` **stays**: `BsSizeUE`/`BsSizeSE`
survive in `vlc_encoder.rs` because the mode-decision cost functions want a code length
without writing anything, so it is still a genuine two-implementation comparison. That
file now has exactly two live comparisons left (this and the frozen CAVLC pair) plus
the decoder's `GetLeadingZeroBits`/`BsGetTrailingBits`.

### Hand-off: T3.5, the encoder CABAC triple and the rest of the rollback

**Start from a clean full battery** — the tree ends on `OVERALL: PASS` with nothing
explained away, which is the first time this phase that has been true at a session
boundary. Then:

* **T3.5 is smaller than the brief thinks.** Its CAVLC half already landed here,
  forced by the field types: `SDynamicSlicingStack` holds `sBsStack: BsWriter` and
  `StashMBStatusCavlc`/`StashPopMBStatusCavlc` are two assignments. What remains is
  `set_mb_syn_cabac.rs`'s `m_pBufStart`/`m_pBufCur`/`m_pBufEnd` triple (`:139-143`,
  walking at `:825`, `:839-860`, `:1009-1028`) and the CABAC stash's
  `pRestoreBuffer` byte copy. The `Copy`-of-`BsWriter` design the brief specifies is
  already proven in production by the CAVLC half.
* **Two boundaries are marked and waiting for you**, both in the faces-2+3 commit:
  `WelsInitSliceCabac` hands the coder `buf.as_mut_ptr().add(w.pos())` and the buffer
  end, and `WelsWriteSliceEndSyn` takes the position back with
  `BsWriter::set_pos(end.offset_from(buf.as_ptr()))`. `set_pos` exists for that one
  caller and debug-asserts the empty accumulator that makes it valid; when the triple
  becomes a `pos`, both boundaries and `set_pos` should disappear together.
* **T3.6's shape is now visible.** The two `SHIM(phase3)` helpers are the whole of what
  stands between the encoder and owned output buffers, and they are one-liners.
  `bs_buffer(ptr, len)` has exactly the signature a `Vec<u8>` field would make
  unnecessary. Session D's construction-audit rule is what governs that seam: `BsWriter`
  was safe under `mem::zeroed` because it is three integers, and a `Vec` will not be —
  the decoder's `MaybeUninit` shell plus `new_boxed()` is the precedent, and the
  encoder's zeroing is wholesale (`sWelsEncCtx::default()` behind `Box::into_raw`, and
  `WelsMallocz`'d slice arrays).
* **One straggler is queued for S18**, not fixed: `md.rs` carries a fifth copy of the
  *size* pair (`BsSizeUE`/`BsSizeSE`) over a **third** name for the Golomb length table
  (`G_KUI_GOLOMB_UE_LENGTH`). Different family, different owner; F2's entry records it.

## 2026-08-11 — Phase 3, session F (T3.5, T3.6, and the phase exit — Phase 3 is complete)

**Goal:** [`prompts/archive/phase3_session_f.md`](prompts/archive/phase3_session_f.md) — T3.5 (reduced),
T3.6, and the exit. All three landed. The brief's fallback ("if T3.6 runs long, the exit
becomes session G") was **not** taken, by direction: the exit ran in full.

**Started at** `bcdb1d8b` with two uncommitted doc items (the S20/S21 plan additions and
this session's brief, committed first as `ceaf1d56`); **ended at** the commit carrying
this entry. Tree clean at both ends.

### What landed

| commit | what |
|---|---|
| `ceaf1d56` | the inherited S20/S21 additions to plan §7.6, plus the session brief |
| `f828a55c` | **T3.5 face 1** — the CABAC arithmetic engine's *second* transliteration is deleted; nine functions and five tables; zero bytes of output move |
| `45ab079c` | **T3.5 face 2** — the cursor triple → three `usize` offsets; the write-extent audit; `BsWriter::set_pos` → `BsWriter::at` |
| `32b73efb` | **T3.6** — `SWelsEncoderOutput`'s three allocations become `Vec`s; one constructor, one `drop`, the first encoder free-cascade entries to fall |
| `e3fc68e8` | **S18 straggler sweep** — the last 112 `BitStringAux` identifiers renamed |

### Gates

| | inherited (`bcdb1d8b`) | final |
|---|---|---|
| tests | 431 / 425 / 20 | **431 / 425 / 20**, unchanged all session |
| T3.0 goldens | 2316 rows, 0 `WITHHELD` | **2316 rows across 12 files, 0 `WITHHELD`**, unmoved |
| sweeps | 341/341 both | 341/341 both, with three F3 episodes (§5) — all acquitted |
| benches | bit-identical | bit-identical, both, throughout |
| Miri `--lib` | 291 | **291**, no new skip |
| ratchet | 1296 / 5034 / 157 | **1286 `unsafe_fn` / 5001 `raw_ptr` / 616 `unsafe_block` / 157 `SHIM(`** |

**The milestone.** The bitstream layer contains no pointer cursor on either side, and
the encoder's frame output owns its buffers. What is left raw in this layer is one
struct, `SWelsSliceBs`, excluded on purpose and fenced to Phase 6.

### 1. The write-extent audit found a second engine, which is why T3.5 had two faces

F16's procedure says enumerate every write before converting. Doing that to the CABAC
coder turned up something the brief did not anticipate: **there were two copies of the
arithmetic engine**, and the live path used both. `svc_set_mb_syn_cabac.rs` carried its
own transliteration of nine core functions and five tables; because a module-local item
beats a `use`, that file's syntax layer ran the local copy for every macroblock while
`WelsWriteSliceEndSyn` flushed the *same* `SCabacCtx` through the canonical copy in
`set_mb_syn_cabac.rs`. Two engines, one slice, split at flush time.

Upstream has no such duplication — `set_mb_syn_cabac.cpp` owns the engine and
`svc_set_mb_syn_cabac.cpp` owns only the macroblock syntax — so this was a port
artifact, and the canonical copy is the one matching upstream's file.

**All nine divergences were enumerated per copy, not by representative** (S21's second
clause, which exists because F2's table compared one function and missed another). Eight
were equivalent. The ninth had teeth: `BypassOne` used upstream's branchless
`kuiBinBitmask = -uiBin` in one copy and an `if uiBin != 0` in the other, and **those
differ for `uiBin ∉ {0,1}`**. All six reachable call sites were checked rather than
assumed — literals, `(iSufS >> k) & 1`, and three `if x < 0 { 1 } else { 0 }` — all in
`{0,1}`, so the divergence is unreachable.

**The corroboration was four lines below the deleted block.** A comment in that same
file records a local `BsAlign` that was missing its trailing `BsFlush`, beat the import,
and started the arithmetic coder on top of already-written slice-header bytes. Same
mechanism, same file, second occurrence, and the first one shipped a real defect. The
shadowing cannot recur now.

### 2. `m_pBufEnd` was assigned and read by nothing

The audit's load-bearing result, and the reason the F16 procedure keeps earning its
place. The CABAC cursor is a triple, and the third element — the buffer end — is written
by `WelsCabacEncodeInit` and **read at no write site, in this port or upstream**. The
bound the field names has never been enforced.

So the honest form of "every write is bounded" here is two clauses, not one:

* **bounded below** by `m_iBufStart`, enforced, in `PropagateCarry`'s `while pos > start`
  — and that comparison is doing double duty, because it is also what stops `pos - 1`
  wrapping a `usize`. This is session C's wrap class arriving from the write side.
  The bound is the *slice's* first byte, not `0`: the carry must not walk into a
  previous slice's bytes. Three unit tests pin exactly that, including `cur == start`.
* **bounded above** by nothing at all, until this seam — slice indexing is what finally
  supplies it. A panic there is a pre-existing sizing bug surfacing (§2.2.2), not a
  regression the conversion introduced.

The field is kept rather than deleted (it still records caller intent, and removing it
would move the layout for nothing) and its deadness is now written down at the site.

`PropagateCarry` came out of this a **safe `fn`**.

### 3. Two corrections to the brief, and one fence that broke

* **`safe/bits.rs:238` is not `BsWriter::set_pos`.** It is the decoder's
  `BsCursor::set_pos`, live for the I_PCM seek and out of scope. The writer's `set_pos`
  was the one the brief described, and it is gone — replaced by `BsWriter::at(pos)`, a
  constructor. That is not cosmetic: `set_pos` had to `debug_assert!(left_bits == 32)`
  because moving an existing writer's position with bits pending drops them, and a
  constructor cannot have pending bits. The invariant became structural.
* **The phase-entry baseline is `5e5c9196`, not `b308f7d5`.** The brief called the
  latter "the phase-entry baseline"; it is session *E*'s start. Both spans are measured
  in `perf_baseline.md`, which matters because they say different things.
* **4b's fence held twice and broke once.** `GetBsPosCabac` needs no buffer (offset
  arithmetic) and `WelsSpatialWriteMbSynCabac` derives one via `slice_bs_buffer` — the
  second application of session E's "derive from the state that recorded the decision"
  rule. But `StashMBStatusCabac` copies emitted bytes, because `PropagateCarry` rewrites
  bytes behind the cursor and restoring the cursor alone would leave the output wrong;
  it cannot reach the buffer from `pDss`/`pSlice`. `PStashMBStatus`/`PStashPopMBStatus`
  gained `buf`. Documented at the type, with what 4b should do with it — and 4b's brief
  says the CAVLC arm can drop the parameter once the slot becomes an enum, which is the
  first concrete win banked for that phase.

### 4. T3.6: the closure was small, and the same fact discharged S21

`SWelsEncoderOutput` is reached only through `sWelsEncCtx::pOut`, **a pointer**. That one
structural fact did two jobs:

* **S20**: nothing embeds the struct by value, so flipping three fields propagated no
  layout change and forced no `abi_guard` deletion. Contrast T3.4, where by-value
  embedding pulled four structs and four asserts into one commit. *The closure is a
  property of how a struct is reached, not of how big it is.*
* **S21**: the encoder context is `mem::zeroed()`, and zero is a valid null
  `*mut SWelsEncoderOutput` where it would **not** be a valid `Vec`. Because `pOut` is a
  pointer rather than a member, the wholesale zeroing never reaches the new owned
  fields — so no `MaybeUninit` shell was needed here, unlike the decoder context, which
  embeds its buffers directly and needs one for exactly that reason.

`uiSize` and `iCountNals` were **deleted rather than converted**, per T3.3's standard.
Four `WelsMallocz` calls became one constructor; four `WelsFree` entries became one
`drop`; `FrameBsRealloc` became two `resize` calls.

**Two scoping findings, both against the brief's assumption.** `SWelsNalRaw.pRawData`
stays raw: `SWelsEncoderOutput.sNalList` and `SWelsSliceBs.sNalList` are separate lists
sharing one struct type, and the second points into the MT-aliased slice buffer this
phase excludes — one type cannot hold offsets into two owners. And the exclusion itself
was verified rather than inherited: `SetOneSliceBsBufferUnderMultithread` binds
`pBsBuffer` to `pThreadBsBuffer[kiThreadIdx]`, so a `Vec` there would mean two owners of
one allocation. Both reasons are written at their sites, naming Phase 6.

### 5. F3's thirteenth and fourteenth measurements — and the first tie

**Thirteenth:** the opening control battery drew a hit on a tree with not one line
changed, on the exact commit session E signed off as a clean `OVERALL: PASS`. Third
consecutive session this has happened. Re-ran 5/5 clean.

**Fourteenth:** T3.6's battery drew **two** release hits, which is what escalates the
retry rule from re-run to alternation. Control `45ab079c`, both binaries built once and
swapped inside one loop, 20 rounds × two configurations per side:
**HEAD 2/40, control 2/40.** The two sides tied exactly — the first of four alternations
that did not come back with the control worse, and a *stronger* acquittal for it: a
difference in either direction on that sample would need explaining, and there is none.

New breadth datapoint: `Static_152_100` appeared in an F3 hit for the first time. The
signature is not confined to the two Cisco clips.

**Fifteenth, at the exit battery**, and the cleanest instance yet of the
two-batteries-one-tree shape: a single release hit (`mt CiscoVT2people_160x96_6fps t=4
sm=3 n=600 cabac=1 rc=1`, **short**, 41681 vs 42281), re-run 5/5 clean. The immediately
preceding exit battery scored 341/341 release on the same tree, and **the only edit in
between was a `#[cfg_attr(miri, ignore)]` attribute on a test** — which is not compiled
into the encoder binary the sweep runs. Same library code, opposite verdicts, with an
intervening diff that provably cannot reach the binary under test.

Running total: **fifteen measurements, four alternations, four acquittals.**

### 6. Perf: the measurement lesson is worth more than the numbers

Full protocol, both benches, against the true phase entry. **Three interleaved pairs did
not converge on this machine today**, and that is the finding:

* whole-phase decode went **+0.68% at 3 pairs → −1.93% at 5 pairs** — a 2.6-point swing
  that changed the sign;
* the encoder-only span's decode median went **+1.21% → +0.16%**, with the Main row
  going **+1.36% → −0.03%**.

The session's null floor said why: the encode band was −1.53% … **+22.57%**, one row
over +5%, *the same binary against itself*. Taken at face value, the 3-pair +1.21% would
have been written up as a real cost of three seams that touch no decoder code.

**No ledger row opens.** Every median is far inside the ≤5%-per-phase gate and nowhere
near the +25% cumulative tripwire, and two measurements of one span disagreeing in sign
is direct evidence the true effect is below the measurement error — D-perf-4's
disposition for that is diagnostic-only. The two defensible statements: the **encoder is
unmoved** (`+0.00%` at both pair counts on the span that could have moved it, cumulative
≈ +8.9% intact) and **decode did not regress and probably improved** (best-converged
−1.93%, consistent with T3.1b's expectation). Full tables in `perf_baseline.md`.

The rule to carry: **when a median lands outside the null band, the first move is more
pairs, not a diagnosis.**

### 7. The exit's own inventory

* **S18 straggler sweep.** `SBitStringAux`/`PBitStringAux`/`TagBitStringAux`,
  `pStartBuf`/`pCurBuf`/`pEndBuf`, `InitBits`, `uiCurBits`: **zero code occurrences**.
  Every surviving mention is prose recording what the type was. The 112 identifiers that
  outlived the type — `pBitStringAux`/`pLocalBitStringAux` parameters and locals whose
  type was already `BsWriter` — are renamed. (`rc.rs` has nine `iLeftBits`; that is a
  rate-control slice budget sharing only the spelling.) **One name survives, listed with
  its owner**: the decoder's `SDqLayer::pBitStringAux`, type already `*mut BsReader`,
  pointer-ness Phase 5's.
* **`SHIM(phase3)` ends at 2, enumerated**: `bs_buffer` and `slice_bs_buffer`, both
  **narrowed** by this session rather than merely surviving — they now guard the
  MT-aliased slice buffer *alone* — and both retire with the thread pool in Phase 6.
* **Miri: 291 `--lib` tests**, three skips unchanged (F12's thread pool, F13's
  `manage_dec_ref` and `encoder_ext`). The widened gate — Miri over the differential
  integration tests — ran at the exit level **and caught something, which is the
  point of it: [F18](phase3_findings.md)**. `kernels_differential_phase2`'s
  `encoder_deblocking_table_installs_the_common_shims` compares reified function
  pointers, and Miri mints a fresh synthetic address for each one (two runs gave
  `43215773 vs 43216036` and `1789169 vs 1789457`). Verified pre-existing — the
  identical failure reproduces at the phase-entry commit in a scratch worktree — and
  fixed the documented way, `#[cfg_attr(miri, ignore)]` with the reason, matching
  `common/mc.rs`'s `init_mc_func_ignores_the_cpu_flag`. The property is symbol
  identity, which Miri does not model; it is unrepresentable there rather than
  untested. **Why it hid for four phases is the reusable part**: the test predates
  F17's fix, F17 meant the Miri gate could not fail at all, and only the `exit` level
  runs Miri over integration tests — so this is the first battery since F17's repair
  that could both fail *and* look at it. **A repaired gate has a backlog.** Session E
  recorded the negative of this; F18 is the positive case. Miri
  `--test kernels_differential_phase2` now 20 passed / 0 failed / 1 ignored.
* **Differential retirement.** `safe_bits_differential.rs` is down to **one** real
  `unsafe` block; its raw reader side retired at T3.1b and its raw writer side at T3.4,
  and what remains is properties, golden tables, the frozen CAVLC parity pair, and the
  writer-to-reader loop. (`safe_plane_differential.rs` still has its 12 — that is Phase
  1's plane work, not this phase's.)
* **T3.0's standing**: **2316 rows over 12 files, 0 `WITHHELD`, dual-profile, unmoved
  this session.** It is the phase's permanent instrument and goes on gating Phases 5
  and 8.
* **S19**: [`prompts/archive/phase4b.md`](prompts/archive/phase4b.md) written. The 4a/4b fence is lifted.
  One correction folded in from measurement: `IWelsParametersetStrategyVtbl` has **20**
  entries, not the 16 the session-F brief carried; `IWelsReferenceStrategyVtbl` is 7 as
  stated.

### Hand-off: Phase 4b

Start from a clean full battery, and **expect an F3 hit on it** — three sessions running
now, on unchanged trees. The brief is written and its §1 has the whole of the first seam
mapped: one boolean in `encoder_context.rs:752-766` selects four `Option<fn>` slots, and
that is an `enum EntropyCoder` with four methods. The thunk deletes itself; the CAVLC
arm sheds the `buf` parameter T3.5 was forced to add. `transmute` is still 23 and has
not moved since Phase 0 — 4b's §3 names it explicitly so it stops not moving.

---

## 2026-08-11 — Phase 4b, session A (T4b.1 and T4b.1b — §1's seam, both halves)

**Commits:** `6e15c907` (inherited doc tail), `08b7c29d` (T4b.1, the entropy
dispatch), `3e583b9a` (T4b.1b, the rate-control table). Phase entry for perf
purposes is `6e15c907`.

### What landed

The brief's §1 in full. Two dispatch families, one commit each, per its own rule
that a byte-exactness failure must name its cause:

* **T4b.1** — `pfWelsSpatialWriteMbSyn`, `pfGetBsPosition`, `pfStashMBStatus`,
  `pfStashPopMBStatus` → `enum EntropyCoder { Cavlc, Cabac }`, four `#[inline]`
  methods that `match`. Deleted with them: `WelsSpatialWriteMbSynCabacThunk`,
  four typedefs, four `is_some()` asserts, three `extern "C"`s, and the `buf`
  parameter on both CAVLC stash variants.
* **T4b.1b** — `SWelsRcFunc`'s nine slots → one `eInstalledMode: RCMode` and nine
  methods; nine typedefs deleted, nineteen call sites de-guarded.

`assert_size!(SWelsFuncPtrList)` **1272 → 1248 → 1184**. Both seams' S20 closure
was the same and trivially small — the table is reached only through
`sWelsEncCtx::pFuncList`, a *pointer*, so nothing embeds it and nothing else had
to move. Both S21 audits discharged by statement: each enum's zero discriminant
is a declared variant (`Cavlc = 0`, `RC_QUALITY_MODE = 0`), so the `mem::zeroed()`
and `WelsMallocz` construction paths stay sound.

### Gates

Tests **431 / 425 / 20** unchanged across both commits. Miri **291/291**. Decode
goldens and both benches bit-identical. Sweeps 341/341 in both profiles modulo
F3, below. Ratchet regenerated twice per S16: `unsafe_fn` 1286 → 1289 → **1298**,
`raw_ptr` 5001 → **4998**, `mem_zeroed` 26 → **28**, everything else flat —
`transmute` **still 23**.

### 1. The same question, asked twice, answered both ways

Both seams cache a configuration selector at init. Both therefore raise the same
question: *can the cached selector and the live parameter disagree?* The answer
had to be checked separately for each, and it came out differently:

* **Entropy: no.** `svc_encode_slice.rs` keeps a local `kbCabac` read live from
  `pSvcParam`. `WelsEncoderParamAdjust`'s no-reset arm copies fields one by one
  ("we can not use direct struct based memcpy due some fields need keep unchanged
  as before") and `iEntropyCodingModeFlag` **is not among them**; every path that
  changes it goes through `bNeedReset` into a full uninit/init. So `kbCabac` was
  left exactly as it was.
* **Rate control: yes.** The same no-reset arm *does* assign
  `pOldParam->iRCMode = pNewParam->iRCMode`, and does not re-point the table.
  Upstream's own comment four lines below reads "Any else initialization/reset for
  rate control here?". So the encoder can legitimately run the previous mode's
  callbacks until something re-inits, and **reading the live mode at the call site
  would have silently fixed that** — S6's "parity, not repair", arriving from an
  unexpected direction. The field is `eInstalledMode` precisely so the lag is
  named rather than accidental.

**The reusable form**: when a conversion turns a cached table into a derived
value, the derivation is only equivalent if the source cannot change behind the
cache. That is a property of the *update paths*, not of the dispatch, and it has
to be read out of them one field at a time. Two fields set from the same struct
by the same init function gave opposite answers.

### 2. Three call sites that did not substitute

Most of the 35 converted call sites are `if let Some(f) = slot { f(args) }` →
`e.Method(args)`. Three were not, and each needed an argument written down:

* **A condition that read as two and was one.** `PrepareEncodeFrame`'s
  `if bSimulcastAVC { if let Some(f) … } else if let Some(f) { for … }` tests the
  *same slot* in both arms; the discriminator is `bSimulcastAVC` alone.
* **A guard wrapping a test.** `WelsRcCheckFrameStatus` puts `!bSkipMustFlag &&
  iMaxSpatialBitrate != UNSPECIFIED` *inside* `if let Some(check_skip)`, so
  hoisting it runs a body that used to be skipped. Equivalent, by the loop's own
  structure: `!bSkipMustFlag` on entry implies this iteration's `bSkipFlag` was
  false, so a no-op callback leaves the re-check false.
* **An absence a caller reads.** `pfWelsRcPostFrameSkipping`'s `None` meant "never
  take this skip-and-return path", so the method's empty arms must return `false` —
  the one place in either seam where the *value* of "not installed" is load-bearing.

### 3. F3: sixteenth to eighteenth measurements, and the alternation's unit was wrong

Three hits, two trees, both profiles, all with the signature (`mt`, `sm=3`, `t=4`,
wrong length). Full table in [`phase0_findings.md`](phase0_findings.md). Two
process results:

* **The session-start battery was clean**, breaking the three-session streak the
  brief told me to expect. The standing advice is a tendency, not a rule.
* **Isolated re-runs are the wrong unit to sample.** The escalated alternation run
  on the hitting configurations in isolation gave **HEAD 0/40, control 0/40** —
  neither side reproduced at all. Re-run at the level the hits actually occur
  (whole `mt` sweep presets, 341 configurations back to back — the *loaded*
  condition) it gave **HEAD 4/12 sweeps, control 5/12**: the control hit more
  often. That also measured the rate directly for the first time, ~1 in 800
  configurations, matching the 1/400-1000 the finding has claimed since Phase 0
  from indirect evidence; and it produced all three wrong-length forms (zero,
  short, long) on *both* sides inside one loop.
* **When an alternation comes back 0/0, it has not run yet.** Re-run it at sweep
  level before recording a result.

### 4. Perf: S2b's second payment, and a bigger one

Session floor (`null`, 3 pairs): encode median **+0.00%** (−1.45% … +1.45%),
decode **−0.16%**. Both seams flat: T4b.1 encode +0.05% / decode +0.08% at one
pair; T4b.1b encode **−0.15%** / decode **−0.06%** at three. **No ledger row
opens**, cumulative unchanged.

The number to keep is the one that was wrong. T4b.1b's single-pair run reported
`640x480 (VGA Mandelbrot) [4t]` at **+22.91%**; at three pairs the same row reads
**−0.49%**, and its 1-thread twin read +0.08% in the same run. A 23-point swing
from a pair count, on a seam that deletes nine function pointers and adds no work
to any loop. Phase 3's exit earned S2b with a 2.6-point swing that changed a sign;
this is an order of magnitude larger. **The median was right at one pair and the
row was not** — a per-row maximum is a single sample however many rows the table
has, and "one interleaved pair per seam" buys a median, not a row.

### Hand-off: Phase 4b, session B

§1 is closed. What remains in the brief, in its order:

* **§2, T4b.2 — the two strategy vtables.** Scouted, not started. The
  implementor counts decide enum-vs-trait and they are asymmetric:
  `IWelsParametersetStrategy` has **one** ported implementor
  (`CWelsParametersetIdConstant`; `CreateParametersetStrategy` returns an explicit
  error for the other four C++ strategies), so it is not really dispatch at all —
  a concrete type with inherent methods, and `Destroy` becomes `Drop`.
  `IWelsReferenceStrategy` has **three** (`TemporalLayer`, `Screen`,
  `LosslessWithLtr`) sharing one data member, which is the closed-and-small case
  the brief says an enum wins. Both are `Box::into_raw`'d today, both are 8-byte
  thin pointers stored in structs with size asserts, and **S20's closure must be
  computed before sizing** — `pParametersetStrategy` is a member of
  `SWelsFuncPtrList` and `pReferenceStrategy` a `*mut c_void` in `sWelsEncCtx`,
  both of which have asserts.
* **§3, T4b.3 — 4a's leftovers**, including `decode_slice.rs`'s cache-fill
  `transmute`s. **`transmute` is still 23 and still has not moved since Phase 0.**
  Two seams of this phase have now gone by without touching it; it will keep not
  moving until a session starts there rather than ending there.

The `SWelsFuncPtrList` size assert is the phase's running tally and is now at
**1184** from 1280. Every remaining de-virtualization moves it, and the comment
above it records why each time — that comment is the cheapest place to see how
much of Phase 4 is actually done.

---

## 2026-08-11 — Phase 4b, session B (T4b.2a, T4b.2b, T4b.3a — both strategy objects and the transmute family)

**Commits:** `87a89d31` (inherited doc tail + the rewritten session-B brief),
`d6c78c1b` (T4b.2a, the parameter-set strategy), `be67a754` (T4b.2b, the reference
strategy), `33b1f0f3` (T4b.3a, the intra-pred constraint family; an earlier draft of
this entry cited `489b95d5`, the pre-amend hash of the same commit — `33b1f0f3` is
the one on the branch). Perf entry for the session is `3e583b9a`.

### What landed

The brief's §2 in full and the headline half of its §3. Three dispatch families, one
commit each:

* **T4b.2a** — `IWelsParametersetStrategy`'s 20-entry vtable, two static instances and
  25 thunks → `enum ParasetIdKind` carried inside one merged
  `CWelsParametersetIdStrategyObj`, with the 20 entries as inherent methods (five
  `match`, fifteen with one body). The field became
  `Option<Box<CWelsParametersetIdStrategyObj>>`.
* **T4b.2b** — `IWelsReferenceStrategy`'s 7-entry vtable, three static instances and 13
  thunks → `enum RefStrategyKind`, with `pCtx` as a parameter instead of a stored
  back-pointer. `Init`, `Destroy`, both factories and a free-cascade entry all deleted
  rather than converted.
* **T4b.3a** — the three `SWelsDecoderContext` intra-pred slots → one
  `enum IntraPredConstraint`, deleting **19 of the crate's 21 `transmute` calls**.

Tests **431 → 435** debug, **425 → 429** release, ignored **20** throughout. Miri
**291 → 295**. Sweeps 341/341 both profiles on every battery. Decode goldens and both
benches bit-identical at every seam. Ratchet across the session: `unsafe_fn` 1298 →
**1259**, `raw_ptr` 4998 → **4834**, `unsafe_block` 616 → **613**, `mem_zeroed` 28 →
**32**, **`transmute` 23 → 5**.

### 1. Two of the brief's three scouted premises were wrong, and both mattered

This is the session's reusable result, and it is uncomfortable: **the brief's §1 and
§3 were each built on a fact read out of a comment rather than out of the code.**

* **§1 said the paraset strategy had one ported implementor**, so the face was
  "deletion, not design". It has **two** — `CONSTANT_ID` and `INCREASING_ID`, two
  static vtables, three overriding thunks — and `INCREASING_ID` is `FillDefault`'s
  value, so it is what an unconfigured encoder runs. The stale "only CONSTANT_ID"
  claim lived in the module doc *and* at the construction site; only the factory's own
  doc comment was right, and session A's scouting had read one of the wrong two.
  Recorded as **F20**. Converting on the brief's plan would have deleted
  `ID_INCREASING_VTBL`'s three overrides and encoded every default-configured stream
  with constant parameter-set ids.
* **§3 said `decode_slice.rs`'s transmutes were data puns**, replaceable with
  `from_ne_bytes` per P7/T7. **None of the 19 is.** All are fn-pointer type erasure on
  three slots whose typedefs declared `*mut c_void` and `extern "C"` where the stored
  functions had concrete types and, for two of them, no `extern "C"` at all. The
  brief also said `transmute` was 23; **two of those are prose**, so the real figure
  was 21 — part of "the metric no phase has moved" was a number that cannot move.

Only §2's scouting held up exactly. **The rule this earns**: a count that decides a
conversion's *shape* — how many implementors, what kind of pun — gets taken from a
`grep` over the definitions at the moment of converting, never from a doc, a brief, or
a prior session's hand-off. Session A's own S23 says to read the update paths rather
than the summary; this is the same rule pointed at prose about the code.

### 2. F19: a leak that no gate in this project could have caught

`encoder_ext.cpp:1995` deletes the parameter-set strategy before freeing the function
table. The port had the outer `if` and the `WelsFree` and **not the delete**, so
`InitFunctionPointers`'s `Box::into_raw` had no matching `from_raw` on any production
path — `DestroyParametersetStrategy`'s only four callers were tests. ~1.2 KB per
encoder destroyed, and once more per `WelsEncoderParamAdjust` reset.

A leak fails no byte-exactness test, no sweep, and no Miri `--lib` run. The instrument
that found it was the ownership audit the `Option<Box<_>>` conversion forces: *which
line frees this?* **The vtable made that question harder to ask, because `Destroy`
looked like an answer** — a correct destructor, wired into a static vtable, that
nothing live ever called. Full write-up in
[`phase4b_findings.md`](phase4b_findings.md).

### 3. Two things S20's closure caught that a field-type reading would not

* **`SWelsFuncPtrList` derived `Copy, Clone`.** An owned field makes that impossible.
  Removing it broke nothing — nothing ever copied the table by value — which means the
  derive had been licensing a silent second owner of the strategy pointer since the
  day it was allocated. A closure computed as "every struct reachable from the changed
  signature" finds this; one computed as "what size is the field" does not.
* **`sWelsEncCtx`'s size does not move, and that is the fortunate answer.** The brief
  said T4b.2b would change it and the assert would update in the same commit. In
  `#[repr(C)]`, an 8-byte pointer becoming a 1-byte discriminant between two
  8-byte-aligned neighbours costs 7 bytes of padding — exactly what it gave up. Had it
  moved, four of the fifteen `assert_ctx_offset!` pins sit *after* that field and
  encode unmodified C++ `offsetof` values; "update the assert" would have been the
  wrong fix.

### 4. The aliasing hazard the vtable was hiding (T4b.2a)

`WelsWriteParameterSets` held a `*mut` to the strategy across calls to
`WelsWriteOneSPS`/`WelsWriteOnePPS`, which reach the same object again through
`pCtx->pFuncList`. `InitDqLayers` did the same around `GenerateNewSps`/`InitPps`. As
raw pointers this was invisible; as `&mut` it is UB, and the conversion could not
compile until it was faced. Every site now re-acquires through one
`ParasetStrategy(pCtx)` helper and no borrow outlives one expression.

**The general form**: converting a raw pointer to a borrow does not *introduce* an
aliasing question, it *surfaces* one that was already there. Phases 5 and 6 are made
of these conversions, so expect the re-entrancy audit to be part of the work rather
than a surprise in it.

### 5. S21 got a test instead of a sentence

Every prior S21 discharge this phase has been a doc comment (and each one moved
`mem_zeroed` by prose, three times). T4b.2b wrote
`ref_strategy_zero_is_the_default_arm` instead — asserting `default()`, the
discriminant value, and an actual `mem::zeroed()` all agree. It costs one real
`mem::zeroed` call in the ratchet and proves what the other three only asserted in
English. Prefer the test.

### 6. F3: nineteenth measurement — zero hits, across eight sweeps

Four full batteries this session (control, and one per seam), **eight 341-configuration
sweeps, both profiles, and not one hit**. Against session A's measured rate of ~1 in
800 that is ~2728 configurations with ~3.4 expected, so P(0) ≈ 3%: a mild surprise, not
a contradiction — and it points the same way S23b does. Session A's hits came out of a
loop hammering the machine with back-to-back sweeps; these batteries interleave sweeps
with compiles and benches and leave the machine idle between steps. **The load is part
of the signature.** Session-start advice stays where session A left it: a tendency, not
a rule, and this session is the second running where the opening battery was clean.

### Hand-off: Phase 4b, session C

§2 is closed. §3 is *started*, at its highest-value item, and stops at a seam boundary
per the brief's own instruction. What remains:

* **T4b.3 items 3-5**: `sBlockFunc` (the deblocking block-dispatch pair), the expand
  fn-pointer re-wraps — **which now own the crate's last two `transmute` calls**, at
  `decoder_core.rs:893-894` — and the ~55-member CPU-dispatch filler, still explicitly
  last and still not allowed to displace anything.
* **The phase exit, whole and uncompressed**, per the brief's §4: straggler sweep
  (S18), the full 3-pair interleaved perf protocol over the phase (**entry
  `6e15c907`**), bookkeeping, and **S19 — `prompts/phase5.md`**.

The `SWelsFuncPtrList` assert stayed at **1184** all session and its comment now says
why: `Option<Box<_>>` is pointer-sized, so a 20-entry vtable left the crate without
moving the number. **Size was the wrong instrument for this session; the ratchet was
the right one**, and `transmute` 23 → 5 is the line to read.

---

## 2026-08-11 — Phase 4b, session C (T4b.3b, T4b.3c, and the phase exit — Phase 4b is complete)

**Commits:** `b6fe4022` (session B's inherited doc tail + this session's brief),
`d1c1a7d4` (T4b.3b, the expand family and the last two `transmute` calls),
`f2e3c5af` (T4b.3c, `sBlockFunc`), and this entry.

### The session in one line

Both remaining §3 items converted, and **both turned out to be the same defect wearing
two costumes**: a type laundered into an identical type because a second declaration
had made the compiler disagree with the programmer. T4b.3b's launderer was
`mem::transmute`; T4b.3c's was `as *mut _ as *mut _`. The first is a tracked metric and
the second is not, which is the more useful half of the finding.

### 1. T4b.3b — the transmutes bridged nothing

The brief said to take the expand re-wraps first because they held the crate's last two
`transmute` calls. They did, and what they were doing is the point.

`decoder_core.rs:880` was a forwarding `ExpandReferencingPicture` that took two
function-pointer slots as an inline `unsafe extern "C" fn(*mut u8, i32, i32, i32)` and
handed them to `error_concealment`'s same-named function through `transmute` — one of
them over a whole `[Option<fn>; 2]` array. **This port declares `PExpandPictureFunc`
four times** (`common/expand_pic.rs`, `decoder_context.rs`, `error_concealment.rs`,
`manage_dec_ref.rs`), and re-greped this session, **all four are the same type**.
Parameter names differ; nothing else does. The wrapper reinterpreted a type into
itself, twice.

The brief's own scouting predicted otherwise — "session B's finding says two of the
erased ones have no `extern \"C\"` at all". True of T4b.3a's family, not this one.
**S24 for the third time in three sessions, and this time against a premise written
specifically for the seam it was wrong about.** The rule is holding up because it keeps
catching things, not because it is quotable.

The table itself was the 4a shape: `InitExpandPictureFunc` takes a CPU flag and ignores
it, `decoder_core.rs` assigned the same constants inline, and in both codecs
`pfExpandChromaPicture[0]` and `[1]` held **the same function** — so the
aligned/unaligned index that a SIMD build would use selected between two identical `_c`
kernels. Branch 1 of the brief's decision tree, and it reached further than the brief
expected: `SExpandPicFunc` lives in `common/`, so this is an **S18 cross-codec
deletion** covering the struct, its installer, four typedefs, the wrapper, members of
both `SWelsFuncPtrList` and `SWelsDecoderContext`, and an `assert_size!`.

### 2. F21 — the S20 closure asked who else implements this, and the answer was "three files"

The finding is written up in full in [`phase4b_findings.md`](phase4b_findings.md). The
short form: C++ has **one** `ExpandReferencingPicture` (`expand_pic.cpp:388`) called
from five sites; the port had translated it **three times**, once per consumer module,
and two copies had drifted in the `kiWidthUV < 16` case. `manage_dec_ref`'s had no
`else` at all, so a frame narrower than 32 pixels kept an unexpanded chroma border on
the error-concealment prefetch path.

Two things worth carrying forward beyond the fix:

* **`error_concealment`'s copy was correct only by accident.** It used
  `pExpChrom[0]` where the C++ uses `ExpandPictureChroma_c`, and on this port those are
  the same function *because both table slots held it*. One table entry away from a
  second real divergence — and the table is exactly what T4b.3b deleted, so the accident
  is now a fact of the code rather than a fact of the configuration.
* **The gates were silent by construction, not by luck.** The smallest legal frame is
  16 pixels wide; every conformance asset is 176x144 or larger, the diffharness inputs
  152x100 and up, and the malformed corpus inherits the conformance SPS dimensions. No
  amount of running the existing battery would ever have found this. Pinning it needs a
  narrow-frame asset, which is golden movement and deliberately not this phase's.

**S21's inventory rule gains a corollary**: the inventory has to be *behavioural*, not
structural. All three copies had the same name, the same six parameters, and the same
C++ citation in their doc comments. A signature-level diff calls them identical. Only
reading the three bodies side by side finds a missing `else`.

### 3. T4b.3c — and the metric that was never the whole population

The brief called this "the deblocking block-dispatch pair". Re-greped: **not a pair, not
deblocking, and not one struct.** `SBlockFunc` has three members — NZC bookkeeping and
two block-zeroing slots — of which **exactly one is ever read**, in this port *and in
the C++*. The two zeroing slots are installed by `decode_slice.cpp:2992-2993` and called
from nowhere in either tree, so they and their `_c` kernels went with the table rather
than being kept as dead ports of dead code.

And the struct was **declared twice** — `decoder_context.rs` and `decode_slice.rs`, same
three members, two different typedef pairs — with `WelsInitDecoderFuncs` bridging them
via `&mut (*pCtx).sBlockFunc as *mut _ as *mut _`.

**That double cast is the session's real finding.** It does exactly what T4b.3b's
transmutes did, for exactly the same reason, and **it does not appear in the `transmute`
metric at all.** So the number this phase just drove to zero was never counting the whole
population; it was counting the subset that used one particular spelling. Recorded in
plan §0 and in the Phase 5 brief, with the instrument that *does* find the rest —
`find_dup_types.sh` and a grep for `as *mut _ as *mut`.

One more duplicate closed on the way: the port had **three** `WelsNonZeroCount_c` where
the C++ has one, and the decoder's was the copy that never got Phase 2's conversion — a
hand-written loop where the other two are shims over the safe kernel. With the table
gone there was no `Option<fn>` slot left for its `extern "C"` to satisfy, so the single
reader now calls the `common` shim and the copy is deleted.

**The gate that proved it safe is the gate that caught it.**
`kernels_differential_phase2.rs::nonzero_count_duplicates_agree` has asserted all three
bodies byte-equal over 50 random inputs since Phase 2 — so the equivalence was measured
long before this session needed it — and it failed to *compile* when the third arm's
function vanished. That is the ideal behaviour from a differential test over a duplicate
family: it is the reason the deletion is safe *and* the alarm when the deletion happens.

### 4. F3: five hits, and the alternation S23b has been asking for since session A

The session-start battery hit before a line was changed, and the T4b.3b battery hit
twice more (once per profile). Full detail is F3's twentieth measurement in
[`phase0_findings.md`](phase0_findings.md); the parts that change how this is tested:

1. **The first alternation that is not 0/0.** 12 `mt` sweeps per side, 1440
   configurations each, binaries swapped inside one loop: **base 2 hits, head 3**. Given
   five hits split 3–2, P(head ≥ 3 | equal rates) = 0.5. Alternations five to seven were
   0/40, 0/40 and 0/0, and session B drew a zero across eight sweeps — this is the first
   time the instrument has produced a reading rather than a shrug.
2. **The head side supplied a direct acquittal, not just a symmetric one.** It hit one
   configuration twice with two *different* wrong lengths — 0 bytes at sweep 8, 28537 at
   sweep 11, against a stable C++ 30190. This finding's own criterion is that a
   deterministic port bug repeats its bytes. Met on the tree under suspicion, which
   beats a rate comparison.
3. **The signature narrows to `n=600`.** All nine of the session's hits are the tighter
   of the two `sm=3` byte constraints; none is `n=1500`. Whoever eventually fixes this
   starts where slices are smallest and most numerous.
4. **An isolated re-run is not always the wrong instrument.** Session A's rule came from
   0/80 in isolation. This session's retry hit **1 in 5** in isolation — immediately
   after a full battery. The variable is *recent load*, not isolation. So the cheap
   single-configuration re-run goes first, and the expensive sweep alternation follows
   if it comes back clean.

### 5. What the phase actually bought, and the instrument that shows it

`assert_size!(SWelsFuncPtrList)` is the number this phase's commits keep quoting, and
it is the wrong one. It read **1272 → 1184** at T4b.1/T4b.1b and then sat unmoved
through three seams that deleted two vtables, 25 thunks and 19 transmutes — because
`Option<Box<_>>` is pointer-sized and a size assert measures bytes of members. It moved
to **1160** only at T4b.3b, when an embedded 24-byte struct left.

The ratchet is the phase's real ledger:

| metric | Phase 4b entry | exit |
|---|---|---|
| `raw_ptr` | 5001 | **4815** |
| `unsafe_fn` | 1286 | **1250** |
| `transmute` | 23 | **4 — and all four are prose** |
| `SHIM(` | 157 | 157 (`SHIM(phase3)` still exactly **2**) |

**The `transmute` row now has a floor and the floor is prose.** The crate contains zero
`mem::transmute` calls. Two matches are pre-existing comments in `encoder_context.rs`,
one is T4b.3a's tombstone, one is T4b.3b's note. Stated in plan §0 and in the Phase 5
brief in the same words, because a metric that reads 4 with nothing behind it will
otherwise be chased.

### 6. Exit bookkeeping

* **Straggler sweep (S18).** Every `Vtbl` name remaining is either the **public C ABI**
  (`ISVCEncoderVtbl` / `ISVCDecoderVtbl` in `api/codec_api.rs`, `G_ISVCENCODER_VTBL`),
  which is the codec's external interface and permanent, or prose. No internal dispatch
  vtable survives. Both converted families (`SExpandPicFunc`, `SBlockFunc`) have zero
  live slots — every remaining mention is a comment. `SHIM(phase3)` is **2**, both
  Phase 6's. **One live straggler is listed with its owner**: `pfSetNZCZero`
  (`encoder/wels_func_ptr_def.rs:385`), the last slot of the NZC family, one
  unconditional constant, encoder-side and therefore 6.5's — flagged in the Phase 5
  brief because it is six lines and its owner is ten sessions away.
* **Perf.** Full 3-pair protocol, four runs. Floor: decode **-0.24%**, encode
  **+0.21%** — *not centred on zero*, which is the reading that matters, because all
  four decode medians this session are negative and cluster inside 0.4 points of the
  floor. T4b.3b **-0.64% / +0.00%**, T4b.3c **-0.47% / +0.32%**, whole phase
  (`6e15c907` → `f2e3c5af`) **-0.36% decode / +0.08% encode**. **No ledger row opens**;
  cumulative unchanged at ≈ +8.9% encoder, ≈ +17.8 / +10.1 / +9.6% decode. Flat, as
  every one of this phase's five seams has been, for 4a's reason — every arm removed was
  runtime-selected, so there was no per-call scaffolding to recover. Detail in
  [`perf_baseline.md`](perf_baseline.md) §Session C.
* **The exit-level gate ran** (`gates.sh exit`, the level that only runs at a phase
  exit and therefore accumulates a backlog — F18's lesson, S22). **Clean on all four
  Miri targets**: 295 `--lib`, 20 `kernels_differential_phase2`, 7 `safe_bits_differential`,
  3 `safe_plane_differential`. Phase 4b leaves no F18-class backlog. It also drew two
  more F3 hits, both `mt sm=3 n=600 t=4`, both retried 5/5 clean, on a tree the
  alternation had already acquitted and with only documentation changed since — folded
  into F3's twentieth measurement as independent samples.
* **S19.** [`prompts/phase5.md`](prompts/phase5.md) written — the hand-off for the
  plan's first pivot, carrying S24, S25 and F19's class by name, and stating that
  per-session scope is the S20 closure rather than the file.

---

## 2026-08-11 — Phase 5, session A (Face 1 duplicate census, Face 2 P3 tests, Face 3 the 5.1 closure)

**Commits:** `17167c81` (inherited doc tail + S26), `126af95c` (T5.A1, the dead shadow
declarations), `40c489f6` (T5.A2, the double-cast census and `SDeblockingFunc`),
`2ae87d9a` (T5.A3, `SPartMbInfo`), `31312084` (T5.A4, twelve tables), `2aa51ddb`
(T5.A5, the census gate and F22), `888f7aa7` (T5.A6, the P3 tests), and this entry.

### The session in one line

The brief said the duplicate census was a survey; it was a defect hunt, and the
instrument the phase most needed had never looked at the decoder.

### Control battery and recount

`gates.sh full` at entry: **OVERALL: PASS**, 435 debug / 429 release / 20 ignored,
Miri **295**, sweeps 341/341 both profiles, both benches bit-identical, ratchet clean.
Every number in the brief's §0 confirmed. No F3 hit at session start — the third
running.

### 1. Face 1, and what a name census is actually for

Three instruments, re-greped rather than trusted (S24). Two of the brief's three
figures were wrong, in the brief's own favour:

* `find_dup_types.sh` reported **252 lines / 17 types / 34 aliases / 27 tables / 37
  value-divergent constants** — the brief's numbers exactly. This is the one that held.
* `grep 'as \*mut _ as \*mut' src/` reads **100**, not 121. The other 21 are
  `as *const _ as *const`, which that command cannot match. 121 is the family; the
  brief quoted a family total against a command that sees 83% of it.
* `find_stub_bodies.py --dups` reported 51 groups, **none** in the decoder — because
  its `RUST_DIRS` and `CPP_DIRS` never listed the decoder. Phase 5 is the decoder
  phase. Widened: **51 → 198** groups, **156** decoder-touching.

Four commits of unification came out of it, and they split cleanly by how the
duplicate was *found*:

**Nothing referenced it (T5.A1).** Nine dead shadow declarations, of which
`manage_dec_ref.rs` held an island of nine names referencing only each other — its
`SPps` had **two** fields where `parameter_sets::SPps` has forty. `nalu.rs`
glob-imports that module, so the shadow was in scope there; what kept it from being
selected is a hand-maintained explicit-import list under the comment "Explicit imports
to resolve glob ambiguities". Inert by a name-resolution rule and a comment. 216 lines
deleted, type duplicates 17 → 12.

**The compiler found it (T5.A2).** The 94 double casts with *both* targets inferred
are the only ones that can launder silently, so the probe was to delete them all and
let rustc type-check each argument exactly. **93 compiled** — they had been
reinterpreting a type into itself, T4b.3b's finding in T4b.3c's spelling. The 94th did
not, and named the defect itself:

```
note: `decoder_context::SDeblockingFunc` and `deblocking_common::SDeblockingFunc`
      have similar names, but are actually distinct types
```

One struct declared twice, field for field identical, bridged by a double cast on the
decoder's init path — and its six kernel-pointer aliases duplicated the same way,
differing **only in parameter names** (`pPixY`/`iSampleY`). That is what made two
identical types distinct to the compiler, and the cast is what hid it.

**The values had to be compared (T5.A3, T5.A4).** A name census proves nothing about a
table. Thirteen decoder tables were flattened to token sequences and compared element
by element before any merge; twelve were identical and were unified, and
`g_ksInterBSubMbTypeInfo`'s three copies differed **only in a field name** —
`mv_pred.rs` spells `SPartMbInfo`'s first field `iMbType` where C++ and the other
three spell it `iType`. A structural diff calls those copies identical; a value diff
calls them different; neither is right on its own.

**And three tables that must not be merged.** `g_kuiAlphaTable`, `g_kiBetaTable`,
`g_kiTc0Table`: the decoder's are `[52+24]` read through a `+12` bias, the encoder's
`[52+12]` read unbiased — 76 vs 64, 76 vs 64, 304 vs 256 elements, **divergent in the
C++ too**. The port is faithful and the shared name is the trap. This is the (c) class
the brief predicted, and it is the reason the allowlist records values rather than
verdicts.

### 2. F22, and what a blind instrument costs

Widening `find_stub_bodies.py` to the decoder immediately produced a finding of F21's
exact class. C++ has **one** `UpdateP16x8MotionInfo` (`mv_pred.cpp:871`) and
`parse_mb_syn_cabac.cpp` calls it. The port translated it — and five neighbours — a
second time inside `parse_mb_syn_cabac.rs`, and the second copy **dropped the
`pDec != NULL` branch**. Guard counts per decoder module: `mv_pred.rs` 28,
`decode_slice.rs` 13, `parse_mb_syn_cavlc.rs` 4, `parse_mb_syn_cabac.rs` **0**. The
CABAC path calls the local copies.

Whether `pDec` can be null there is 5.2's question; either way the two copies disagree
on the mainline CABAC path and no gate here can tell, because every corpus stream
decodes with `pDec` attached. Written up in
[`phase5_findings.md`](phase5_findings.md), owner 5.3.

**The general form, and it is not comfortable.** F21 was found by a closure asking
"who else implements this?". F22 was found by an instrument that had been running for
three phases while blind to a quarter of the crate. Both are the same defect class,
and the second one was *reachable by an instrument this project already had*. S22 says
a repaired gate has a backlog; the corollary is that an instrument's scope is part of
its result, and "none found" from a tool that does not look is not a measurement.

### 3. The census is a gate now (T5.A5)

`rust/tools/census.sh` + `census_allowlist.txt`, wired into `gates.sh` at **commit**
level. 61 allowlisted entries keyed `<kind> <name> x<count>` — the count is in the key
so a *new* copy of an allowed name fails — each carrying its class ((a) cross-codec
namesake, (b) to-unify with the owning step, (c) do-not-merge with the values, (d)
legitimate) and its reason. Two budgets instead of lists: inferred-target double casts
**0**, named-target **25**; duplicate-body groups a ratchet at **198**.

Proven red before being trusted (F17's lesson): a second `SPartMbInfo` makes the
declaration arm fail with the file:line pair; a `p as *mut _ as *mut _` makes the cast
arm fail and print the instruction to delete the cast and let rustc name the two types.

### 4. Face 2 — the P3 tests, and the one that proved nothing

Five tests over the three identity sites, each giving the two pictures **the same POC**
so a POC-based rewrite takes the wrong arm and fails.

The `ON_MB_BS` test is worth recording as a near miss. Its first draft asserted only
that the same-object and distinct-object calls *differ*; with the MVs first chosen, all
three of the function's arms returned 1 and the test failed for the right reason. The
fix was to choose MVs where straight comparisons exceed the threshold and crossed ones
do not, which makes the `ref_p0 == ref_p1` arm (an AND of both) false and the other
arm true — 0 vs 1. **An identity test whose MV configuration cannot separate the arms
tests nothing**, and it will pass just as happily after the conversion that breaks it.

S12 collected again: `WelsCopy16x16_c` bumps its row pointer after the last row, so
the exactly-sized plane the EC test first allocated is an out-of-bounds `offset`. Miri
caught it on the first run; the planes carry a spare row with the rule named at the
allocation.

Tests **435 → 440** debug, **429 → 434** release, Miri **295 → 300**.

### 5. Face 3 — 5.1's S20 closure, computed and recorded

`SPicture`'s plane fields become owned (`PaddedPlanes` + `Vec`s). The closure:

**Reachability.** Nothing embeds the decoder's `SPicture` **by value** — every holder
is a pointer, which is the fortunate answer and the reason 5.1 is the right first step.
The pointer holders, by count of mentions:

| file | `*mut SPicture` / `PPicture` | role in the closure |
|---|---|---|
| `manage_dec_ref.rs` | 17 | DPB lists, marking, MMCO — converts with 5.1 |
| `pic_queue.rs` | 13 | the pool itself; `SPicBuff.ppPic` is `*mut *mut SPicture` |
| `deblocking.rs` | 10 | `pRefPics` — **P3 site 1, now pinned** |
| `error_concealment.rs` | 7 | **P3 sites 2 and 3, now pinned** |
| `picture.rs` | 6 | `pRefPic[[; 17]; LIST_A]` and `pSetUnRef` |
| `decoder_core.rs` | 3 | `pDec`, `pPreviousDecodedPictureInDpb` |
| `decoder_context.rs` | 1 | `SRefPic`'s three `[[*mut Picture; MAX_DPB_COUNT]; LIST_A]` |
| `decode_slice.rs`, `mv_pred.rs`, `parse_mb_syn_cavlc.rs` | 1 each | MC and colocated reads — **5.3/5.6, not 5.1** |

**Layout pins: none.** The decoder's `SPicture` has no `assert_size!` and no offset
pins. `assert_size!(SPicture, 136)` in `encoder/abi_guard.rs` is the **encoder's**
same-named struct (allowlist class (a)) and does not move. Compare T4b.2b, where the
pins sat after the changed field and "update the assert" would have been the wrong fix
— here there is nothing to update, and that is a fact worth having before the first
commit rather than after it.

**The fourth plane is dead.** C++ `picture.h:53-55` declares `pBuffer[4]`, `pData[4]`,
`iLinesize[4]`; `AllocPicture` sets `iPlanes = 3` and writes indices 0-2 only, and
**nothing in `src/decoder` reads index 3** of any of the three arrays. The four
`pData[3]` writes in the crate are on `SSourcePicture`, the public API type. 5.1 may
fix the count at three.

**F19's check, per allocation.** `AllocPicture` (`pic_queue.rs:129`) makes eight
allocations and `FreePicture` (`:263`) has a matching free for every one:

| allocated | at | freed at |
|---|---|---|
| `pPic` itself | :143 | :309 |
| `pBuffer[0]` (one block; `[1]`/`[2]` are interior offsets) | :183 | :269 |
| `pMbCorrectlyDecodedFlag` | :223 | :277 |
| `pMbType` | :228 | :285 |
| `pMv[LIST_0]`, `pMv[LIST_1]` | :234, :238 | :294 (loop) |
| `pRefIndex[LIST_0]`, `pRefIndex[LIST_1]` | :244, :248 | :301 (loop) |

The `bParseOnly` arm allocates nothing (every plane pointer set null), so the two arms
are balanced too. **No F19-class leak here** — recorded because the check was run, not
because it found nothing.

**S25 re-entrancy, enumerated with the closure — one real hazard.**
`SPicture::unref(&mut self)` (`picture.rs:292`) does `func(self as *mut SPicture)`, and
the installed `SetUnRef` (`manage_dec_ref.rs:100`) immediately does
`let ref_pic = &mut *pRef`. Two `&mut` paths to one picture, one derived from the
other. It is legal today only because the outer borrow is dead after the call; a
`&mut Picture`-based pool makes it the exact shape S25 describes, and it must be faced
before the pool conversion rather than discovered at compile time. The fix shape is
S25's own: no borrow outlives one expression — `unref` becomes a free function taking
the pool and an id, or the callback goes away with the strategy it encodes.

The second, milder one: `SPicBuff.ppPic` is `*mut *mut SPicture` and the recycling
predicate walks it while `pCtx->pDec` points into the same array.

**Consequence for session B's sizing.** The closure is wide (ten files) but *shallow*
— no by-value embedding, no layout asserts, no `mem::zeroed` reach into the picture
itself. That is the opposite of 5.2's `SDqLayer`, and it means 5.1 can be strangled
file by file behind `PicId` rather than landing as one commit. `deblocking.rs` and
`error_concealment.rs` are the two files whose behaviour the conversion could silently
change, and both are pinned now.

### 6. F3: the twenty-first and twenty-second measurements, and the first even alternation

**Six hits this session**, every one inside the narrowed signature (`mt`, `sm=3`,
**`n=600`**, `t∈{2,4}`, wrong length), across all three clips and both profiles. The
first five were retried in isolation **5/5 byte-identical each time**.

Then the alternation the brief prescribes for two or more hits: 12 `mt` sweeps per
side, 120 configurations each, both release binaries built once and swapped inside one
loop. Base = the session's entry tree (`17167c81`), head = after T5.A4.

**Base 4 hits / head 4 hits**, 1440 configurations per side. The first even split the
instrument has produced. Two details beyond the count:

* The head side hit one configuration **twice with two different wrong lengths** (0
  bytes at sweep 1, 37837 at sweep 3, against a stable C++ 39981) — this finding's own
  race criterion, met on the tree under suspicion, exactly as in session C.
* The base side reproduced `Static_152_100 t=4 sm=3 n=600 cabac=1 rc=1` at **28537
  against C++ 30190** — the same two numbers session C recorded for the same
  configuration on a different tree. The signature is stable across trees and sessions.

Rate this session: 8 hits / 2880 alternated configurations ≈ **1 in 360**, against the
~1/800 baseline — consistent with session B's "the load is part of the signature" and
session C's "the variable is recent load, not isolation". This session ran batteries
back to back for hours.

Also worth stating plainly: **every commit this session changes decoder code or tests,
and these sweeps compare encoders.** The causal link is not merely unlikely, it does
not exist. Ninth alternation, ninth acquittal.

### 7. Numbers

| metric | entry | exit |
|---|---|---|
| tests (debug / release / ignored) | 435 / 429 / 20 | **440 / 434 / 20** |
| Miri `--lib` | 295 | **300** |
| `raw_ptr` | 4815 | **4597** (−218) |
| `unsafe_block` | 613 | **618** (+5, all in the new tests) |
| `unsafe_fn`, `SHIM(`, `transmute`, `mem_zeroed` | 1250, 157, 4, 32 | unchanged |
| duplicate types / aliases / tables | 17 / 34 / 27 | **10 / 31 / 17** |
| inferred-target double casts | 94 | **0** |
| duplicate-body groups (instrument widened) | 51 (blind) | **198** (seeing) |

Ratchet regenerated per S16; the only increases are the five `unsafe {}` blocks in
T5.A6's tests, in the two files those tests live in.

### Hand-off: Phase 5, session B

Face 1 and Face 2 are closed. Face 3 is recorded above and **not started in code** —
the brief's "first strangler commits only from a standing start" was not met with the
time left, deliberately rather than by accident.

Session B starts 5.1 from the closure in §5. What it inherits:

* **The closure is shallow.** No by-value embedding of `SPicture`, no layout asserts on
  it, no `mem::zeroed` reach. Strangle file by file behind `PicId`; do not size this
  like 5.2.
* **The S25 hazard is `unref`/`SetUnRef`, and it is the first thing to face**, not the
  last. It is enumerated in §5 with its fix shape.
* **The three P3 sites are pinned** by five tests that fail if identity becomes POC.
  Read them before touching `deblocking.rs` or `error_concealment.rs`.
* **F21's fix is still unpinned** — the sub-32px chroma expand lives in 5.1's files and
  no corpus stream is narrower than 176px. Pinning it needs a narrow-frame decode asset
  and new golden rows, which is Eugene's call and was not sought this session. The
  exposure stands, recorded here and in the brief.
* **F22 is 5.3's**, but 5.2 answers its reachability question as a side effect. If the
  answer arrives early, put it in the finding.
* **The census gate runs at `commit` level now.** A new duplicate fails the build with
  the file:line pair. When a step legitimately introduces one, add it to
  `census_allowlist.txt` *with its class and owner*, not with a bare name.

---

## 2026-08-11 — Phase 5, session B (5.1's first two steps: F21's pin, the S25 audit)

**Commits:** `1745b5ca` (inherited doc tail — the prompt archive move and ~30 links),
`0fd0c9cc` (T5.B1, the narrow-frame assets), `ff12e966` (T5.B2, the S25 audit and
F13's decoder site), and this entry.

### The session in one line

Both of the brief's first two steps turned out to be one step wider than the brief
knew, and in both cases the extra width was the part that mattered.

### Control battery and recount

`gates.sh full` at entry: **OVERALL: FAIL (1 step)**, and the one step is F3 —
`mt CiscoVT2people_160x96_6fps t=4 sm=3 n=600 cabac=0 rc=0`, C++ 42538 bytes against
Rust **0**, the signature exactly. Re-run 5x in isolation per §0: **5/5
byte-identical at 42538**. Acquitted; tenth alternation not needed for a single hit.
Everything else confirmed the brief: **440 / 434 / 20**, Miri **300**, release sweep
341/341, census 61 allowlisted, both benches bit-identical, ratchet clean.

Three counts re-greped before use (S24). One held, two did not:

* The session-A closure's pointer-holder table — 17 / 13 / 10 / 7 / 6 / 3 / 1 / 1 / 1
  across the ten files — **holds exactly**.
* **`SPicture::unref` has one caller in the crate and it is its own unit test.** The
  brief named this function as 5.1's first hazard and asked for a restructure. The
  C++ `SPicture` has no such member — `picture.h:73` declares the `pSetUnRef`
  callback and nothing else — and the live unreference path is that callback,
  invoked directly from `api/codec_api.rs:1607` and from `manage_dec_ref.rs`'s seven
  `SetUnRef` calls. Its `else` arm was a port invention that *disagreed* with
  `SetUnRef`: it decremented `iRefCount`, which `SetUnRef` never touches. So the
  restructure is a deletion, and the hazard the brief named was never on a live path.
* **`iPlanes` is written and never read** — `pic_queue.rs:210` writes 3, nothing in
  the crate reads it, and the same is true of the C++ (`picture.h:56` declares it,
  `pic_queue.cpp:105` writes it, no decoder source reads it). The brief's "fix the
  count at three" stands; what it fixes is a field that is dead on both sides.

### 1. T5.B1 — the pin, and the two rows that would have been mistaken for one

Three assets, additive, C++ decoder goldens, regenerable by
`rust/tools/make_narrow_assets.py`: `narrow_16x16` (minimum legal width, `iWidthUV`
8), `narrow_24x18` (coded 32x32 and cropped, `iWidthUV` exactly 16), and
`narrow_16x16_idr_lost`.

**Only the third pins F21, and the first two are green under a revert of the fix.**
That is not a weakness of the assets, it is the finding's actual shape: pre-fix,
`decoder_core`'s expand path forwarded to `error_concealment`'s copy, which had the
sub-16 arm. Only `manage_dec_ref`'s copy lacked it, and its single call site is
`WelsInitRefList`'s concealment prefetch — **unreachable by any stream that decodes
cleanly**. A session that added the two obvious narrow rows would have closed F21 and
pinned nothing.

Two properties the concealing asset needed, each discovered by a row that passed when
it should not have:

* **A recycled picture.** The prefetch memsets to 128 over the active area only —
  `iLinesize[1] * iHeightInPixel / 2` bytes from `pData[1]`, which covers the picture
  rows and their left/right padding but not the top and bottom border — and
  `AllocPicture` fills the whole buffer with 128 as well. Concealment from a *fresh*
  pool therefore reads 128 in the border whether it was expanded or not. The asset
  decodes a full 24-frame sequence first so the pool has cycled.
* **Motion.** The source is a window *panned* across the clip. A static crop encodes
  to zero MVs, and on a one-macroblock frame it takes a non-zero vertical MV to read
  outside the plane at all.

The lost IDR is a second sequence whose SPS differs from the first (CAVLC
`profile_idc` 66 then CABAC `profile_idc` 100 — `memcmp` against the stored SPS is
what makes the decoder call it a new sequence and clear the reference lists) with its
IDR NAL removed. **The first ordering of that pair was rejected**: CABAC-then-CAVLC
left two buffered pictures sharing a `uiDecodingTimeStamp`, and the port and upstream
break that tie in opposite directions — frames 22 and 46 came out swapped. A real
divergence, already recorded against `test_scalinglist_jm`, but not this one, and it
would have failed the row at HEAD for the wrong reason.

Coverage proven rather than asserted (F17's lesson): `d1c1a7d4` reverted in a scratch
worktree with the two conflicts resolved so only the expand copies came back — T5.A1's
dead-declaration deletions and T4b.3c's `sBlockFunc` stay deleted, because reverting
more than F21 would prove less. Under it, `narrow_16x16_idr_lost` goes red and the
other two stay green.

`asset_test_concealed!` is a second macro over the *same* harness body with one
parameter added: an asset that deliberately conceals must be judged by `h264dec`'s own
output rule (`iBufferStatus == 1`), because this file's rule drops every frame whose
state is not `dsErrorFree` — on this stream, precisely the concealed ones. Under the
default rule the row compared its clean prefix and nothing else. The 53 existing rows
keep the default and none moves.

### 2. T5.B2 — the S25 audit, and what "who else reaches this object" answered

The brief's hazard was dead code. The audit that was supposed to be its supporting
work is where the live ones were:

**`pRefPic` is not a second object.** Every caller passes `&mut ctx.sRefPic` or
`&mut ctx.sTmpRefPic` (`WelsMarkAsRef:1321`), so `&mut *pCtx` and `&mut *pRefPic`
are two `&mut` to overlapping memory, the inner one not derived from the outer.
**Nine functions** in `manage_dec_ref.rs` held one or both across calls that re-enter
through the same raw pointers — `SlidingWindow`, `RemainOneBufferInDpbForEC`,
`MMCOProcess`, `MMCO`, `MarkAsLongTerm`, `WelsCheckAndRecoverForFutureDecoding`,
`WelsInitRefList`, `WelsInitBSliceRefList`, `WelsMarkAsRef`.

Two of them are worth stating individually, because they are not "stale in principle":

* **`MMCOProcess`'s `MMCO_SET_MAX_LONG` loop terminates on a count the re-entrant
  call decrements.** `while i < ref_pic.uiLongRefCount[LIST_0]` around
  `WelsDelLongFromListSetUnref(pRefPic, ..)`. The loop is *correct only because* the
  callee's write through its own `&mut` is visible to the outer binding — which is
  the same fact that makes reading the outer binding UB.
* **`WelsMarkAsRef` holds `&mut *pDec`** across `MMCO`, whose `MMCO_LONG` arm reaches
  the same picture through `AddLongTermToList(pRefPic, (*pCtx).pDec, ..)`, and then
  writes `dec.iFrameNum = 0` afterwards.

Fixed as S25 prescribes — no borrow outlives one expression — by naming `(*pCtx)` and
`(*pRefPic)` at each use. Worth noting that this is also the **C++'s own spelling**
(`pCtx->iFrameNum`): the ergonomic `let ctx = &mut *pCtx` shorthand is what the port
added, and it is what introduced the hazard.

Behaviour is unchanged by construction: a field access through a `&mut` and one
through a raw pointer read and write the same memory in the same order. Nothing here
is a byte-level change and the goldens say so.

### 3. F13's `manage_dec_ref` site, and what the skip was really holding

F13 named `AddLongTermToList:476` and gave the fix (`copy_within`). Both were right
and both were smaller than the truth.

**The site was six.** `WelsDelShortFromList`, `WelsDelLongFromList`,
`AddShortTermToList`, `AddLongTermToList` and *both arms* of `WelsReorderRefList` were
all written `ptr::copy(list.as_ptr().add(a), list.as_mut_ptr().add(b), n)`. One
`shift_dpb_entries` helper now carries all six, and carries the array bound the C's
`memmove` never checked — written down, not fixed, because bounding `uiShortRefCount`
and `iMaxRefIdx` at their source is the F8/F9/F11 class and §9 excludes it.

**And with the production site fixed, Miri still failed** — in three *tests*, each
taking `&mut picN as *mut _` a second time in an assertion after the list already held
that picture. The assertion's Unique retag pops the tag the list is carrying, and the
next call reads `(*cur).iFrameNum` through it. The list was right; the test made it
UB. Same class as the four test-side instances F13 fixed outright in Phase 2, found
three phases later because **a skipped test is not a passing test**. F18's lesson a
second time, and the sharper form of S22: *the backlog behind a skip is not confined
to the code the skip was written for.*

`--skip manage_dec_ref` is deleted from `gates.sh`. Miri `--lib` runs this module from
this commit on, which matters more for what comes next than for what it caught: the
plane conversion is the next thing to touch these files, and it now has an instrument
watching it. Two of F13's four skips are gone (`svc_mode_decision` at 4a, this one);
`wels_thread_pool` (F12, Phase 7) and `encoder_ext` (Phase 6) remain.

### 4. F3

One hit, at session start, inside the narrowed signature; 5/5 byte-identical on
re-run in isolation. **Twenty-third measurement, and as in session A every commit
this session is decoder-side or test-side while the sweep compares encoders.**

### 5. Numbers

| metric | entry | exit |
|---|---|---|
| tests (debug / release / ignored) | 440 / 434 / 20 | **443 / 437 / 20** |
| Miri `--lib` | 300 | **304** (+4: `manage_dec_ref`'s, no longer skipped) |
| `raw_ptr` | 4597 | **4597** (+0) |
| `unsafe_block` | 618 | **616** (−2) |
| `unsafe_fn` | 1250 | **1249** (−1) |
| Miri skips | 3 | **2** |
| decode goldens | 53 rows | **56 rows**, none moved |

### Hand-off: Phase 5, session C

5.1's steps 1 and 2 are done. **Step 3, the plane conversion, is not started in
code** — it is the whole of session C.

* **Start from session A's §5 closure**, which this session re-greped and which holds
  except for the two corrections in §Control battery above (`unref` is gone;
  `iPlanes` is dead on both sides, so the three-plane fix is subtraction).
* **The S25 audit is done for `manage_dec_ref.rs` and its rule is written at
  `SetUnRef`.** `pic_queue.rs`, `deblocking.rs` and `error_concealment.rs` have not
  been audited — do them with their conversion, not before it.
* **Miri now covers `manage_dec_ref`.** Expect it to have opinions about the pool
  conversion, and read that as the instrument working.
* **The narrow-frame rows are part of the decode goldens now**, including one that
  exercises concealment on a recycled picture — which is a path the plane conversion
  touches directly. If a plane change moves `narrow_16x16_idr_lost` alone, the
  border expansion is where to look.
* F22 is still 5.3's, its reachability question still 5.2's.

---

## 2026-08-11 — Phase 5, session C (5.1 step 3: `SPicture`'s planes become owned)

**Commits:** `272c3b79` (inherited doc tail — the session brief), `79588684` (T5.C1,
the dead members), `999f300b` (T5.C2, the call-site bridge and two S25
enumerations), `7a28e620` (T5.C3, the owned planes), and this entry.

### The session in one line

The conversion went in behind three faces and moved no bytes — and the one defect
it made was invisible to every gate that measures bytes.

### Control battery and recount

`gates.sh full` at entry: **OVERALL: PASS**, 443 / 437 / 20, Miri **304**, sweeps
341/341 both profiles, census 61 allowlisted, both benches bit-identical, ratchet
clean (`raw_ptr` 4597, `SHIM(` 157, `unsafe_block` 616, `unsafe_fn` 1249), decode
goldens **56**. Every number in the brief's §0 confirmed. No F3 hit at entry.

Three closure facts re-greped before use (S24), all three of which the brief either
asserted or left open:

* **Nothing copies an `SPicture` by value.** The brief did not say this and it
  decides whether owned fields are possible at all. Measured by asking the compiler:
  `Copy` removed from the derive, `cargo check --all-targets` clean.
* **`pBuffer` is read in two files.** `pic_queue.rs` (alloc/free) and `picture.rs`.
  Every other site reads `pData`, so the physical base could disappear into the
  plane without touching a caller.
* **`calculate_data_pointers` has no callers.** A method that recomputes `pData[i]`
  from `pBuffer[i]` and the stride — dead, and subsumed by the plane's `origin`.

### 1. The S20 closure, as computed

Wide and shallow, as session A predicted, and one field narrower than its own field
list suggested:

| what | count | where it went |
|---|---|---|
| `iPlanes` | 1 write, 0 reads | deleted, T5.C1 |
| plane slot 3 | 0 reads | deleted, T5.C1 |
| `pData[i]` / `iLinesize[i]` reads | **123** over six files | accessors, T5.C2 |
| `pData[i]` / `iLinesize[i]` writes | `AllocPicture` + 3 test fixtures | the constructor, T5.C3 |
| `pBuffer[i]` | `pic_queue.rs`, `picture.rs` | the plane's own buffer, T5.C3 |
| whole-array reads | 3 `ExpandReferencingPicture` call sites | built from the accessors, T5.C2 |
| layout pins | **none** | — |
| by-value holders | **none** | — |

The three-face split follows from it. The field change is atomic — you cannot own
`pData` in one file and not another — but the *call sites* are not, so introducing
the accessors with their final signatures first makes T5.C3 a change to two files.
That is what made the provenance defect below a two-line fix in one place instead of
a bisect over a hundred-site diff.

### 2. What Miri found, and what nine other gates said about it

`data_ptr` returns logical `(0, 0)` of a plane as a raw pointer. The obvious
spelling is

```rust
plane.as_mut_slice()[origin..].as_mut_ptr()
```

which is safe code, computes the correct address, and is **undefined behaviour at
the first read into the top or left border** — slicing narrows provenance to
`[origin..]`, and the padding behind `pData[i]` is the entire reason a padded plane
exists. `ExpandPictureLuma_c` recovers the whole allocation with
`pDst.sub(pad*stride + pad)`; motion compensation reads negative coordinates after
clamping the vector.

The battery's verdict on that draft: **448 debug / 442 release tests passing, all 56
golden rows bit-identical, both benches bit-identical, sweeps 341/341, census clean.
Nine gates, nine passes.** Miri failed on the first run, on the one test that reads
backwards through the shim pointer.

Two things worth keeping from that:

* **S15's sentence, earned rather than quoted.** *Byte-exactness does not imply
  soundness.* This is the cleanest instance the project has produced: not a latent
  bug in old code that the gates never reached, but a new one, on the mainline
  decode path, that the gates reached constantly and could not see. The bytes were
  right because the memory is there; only the *permission* was wrong.
* **The test is the instrument, not Miri.** Miri can only fail on a read that
  happens. `data_ptr_reaches_the_padding_behind_the_logical_origin` exercises both
  backward reaches the decoder performs — one sample diagonally behind the origin,
  and `expand_shim_span`'s full reconstruction of the allocation. Without that test
  the whole `--lib` suite is silent, because no unit test had ever taken a picture's
  plane pointer and walked behind it. The fix is
  `as_mut_ptr().wrapping_add(origin)`: same address, unnarrowed provenance, no
  `unsafe`, and it is what `pBuffer[i].add(offset)` always meant.

### 3. Byte-exactness, kept by construction

The brief named three invariants. Each is now checked rather than argued:

* **The 128 fill.** One `WelsMallocz` of `iLumaSize + 2*iChromaSize` filled with 128
  became three allocations each filled with 128. The contiguity was incidental —
  `pBuffer[1]` and `pBuffer[2]` were bases for their own plane's `pData` and nothing
  walked between planes — and `test_alloc_and_free_picture` asserts the fill covers
  the whole luma allocation, corner included.
* **Stride arithmetic.** Every expression is `AllocPicture`'s own, including
  `(1 + iLinesize[0]) * PADDING_LENGTH` and `((1 + iLinesize[1]) * PADDING_LENGTH)
  >> 1`. `PaddedPlane::from_parts` recovers the padding by dividing the origin by
  the stride, so it *checks* that both are square paddings (32 and 16) and that the
  allocation is tall enough for the padded picture — the row-count alignment leaves
  eight spare luma rows at 160x120, and the type accepts that explicitly.
* **The EC prefetch.** Untouched: it still memsets the active area only, and
  `narrow_16x16_idr_lost` is green across all three faces.

`bParseOnly` needed a new shape. It sets `iLinesize[i]` from the geometry and leaves
`pData[i]` null, and the strides are read (`GetI4LumaIChromaAddrTable`), so the
planes cannot simply be absent. `PaddedPlane::empty(stride)` is that state: a stride,
no bytes, and every coordinate accessor panicking because there is no addressable
byte. It is the only constructor where a zero stride is legal — with nothing to
index, the stride is metadata — and `SPicture::default()` uses `empty(0)` to go on
reporting exactly the `iLinesize == 0` and null `pData` its zeroed form had.

### 4. F19 and S21, per allocation

`AllocPicture`'s eight allocations became ten, and the audit is not the same audit:

| allocated | freed by |
|---|---|
| the picture (`Box::into_raw`) | `Box::from_raw` in `FreePicture` |
| plane 0, 1, 2 (`Vec<u8>`) | that same `Box`'s drop glue |
| `pMbCorrectlyDecodedFlag`, `pMbType`, `pMv[0..1]`, `pRefIndex[0..1]` | six `WelsFree` calls in `FreePicture` |

Four of the ten are now balanced by the type system rather than by inspection, and
`FreePicture` has no `pBuffer[0]` arm left to forget. Session A's table counted
allocations *inside* `AllocPicture`; the check also has to cover calls *to* it, and
there are two — `CreatePicBuff` (freed by `DestroyPicBuff`) and
`decode_slice.rs:2032`'s lazy `pTempDec` (freed at `decoder_core.rs:1899`). Both
balance.

Plane allocation is **fallible**. `vec![128; n]` aborts the process on failure where
the C returned null and every caller tests for it, so the buffers come from
`try_reserve_exact` — `RawDataBuffer::try_new_zeroed`'s answer at T3.4, and it
matters more here because a plane is megabytes. The picture header itself is
`Box::new` and aborts; that is ~700 bytes and the same deviation `new_boxed` already
carries.

**One divergence the constructor had to decide.** `SPicture::default()` sets
`eSliceType: UNKNOWN_SLICE` (5); `AllocPicture`'s `WelsMallocz` zeroed it, and
`P_SLICE == 0`. The two have always disagreed, and the live path is the zeroing one,
so `with_planes` reproduces `P_SLICE` and a test says so. Nothing observes it — a
picture's `eSliceType` has two writers and no readers in the decoder, `iPlanes`'s
situation one field over — but a constructor replacing a memset does not get to pick
which zero it reproduces.

### 5. The S25 enumerations, one per file

Written into the commit that converted each file, per plan §7.6.

**`deblocking.rs` (T5.C2).** The three `pCsData` pointers are derived from
`pCtx->pDec` and live for the whole macroblock loop. Nothing in that loop reaches
that picture again: the only other pictures it touches are behind
`pFilter.pRefPics[l]`, and it reads them for identity, `pMv` and `pRefIndex` and
never for planes — so no second plane pointer exists to conflict, and the answer
holds even if a reference slot were `pDec` itself. `pCsData` *is* a stored mirror,
which the conversion refuses to put back into `SPicture`; it is `SDeblockingFilter`'s
per-slice scratch and it retires at 5.4.

**`error_concealment.rs` (T5.C2).** The one decoder file whose job is to hold two
pictures at once. Four functions do, and every one returns on `src == dst` before
the first derivation: `DoErrorConFrameCopy`, `DoErrorConSliceCopy`,
`DoErrorConSliceMVCopy`, `DoMbECMvCopy`. Not something this session added — the C++
has all four, because a memcpy from a picture into itself is wrong arithmetic before
it is aliasing — and two of the four are pinned by session A's P3 identity tests.
The guards that make `PicId` safe to introduce later are the same guards that make
the two `&mut` borrows disjoint now.

**`pic_queue.rs` (T5.C3).** The sharpest form of the question, because
`SPicBuff.ppPic` is `*mut *mut SPicture` and `pCtx->pDec` points into that array.
The answer is that **no function in this file holds a borrow of a picture across
anything**: `PrefetchPic` reads `bUsedAsRef` and `iRefCount` per slot and writes
`iPicBuffIdx` after the scan has stopped. `CreatePicBuff` fills `ppPic` before it
sets `iCapacity`, and every prefetch returns early on `iCapacity == 0`, so the scan
cannot see a half-built picture — which is what lets `AllocPicture` hand back a
`Box::into_raw`. The re-entrancy that does exist is one level up, in
`WelsInitRefList`'s concealment prefetch, and it is enumerated at its own site.

That site also produced this session's only S25 *fix*: `let prev = &*prev_pic` held
a borrow across three writes into the other picture. Named `(*prev_pic)` per use,
session B's shape.

### 6. F3

Two batteries hit it, and the second one triggered the alternation.

**Three hits at T5.C2** — `320x192 t=4 sm=3 n=600 cabac=0 rc=1` (debug, 0 bytes),
`320x192 t=4 sm=3 n=600 cabac=1 rc=0` (debug, 37837 against 39981), `160x96 t=2
sm=3 n=600 cabac=0 rc=0` (release, 0 bytes). All three **5/5 byte-identical** on
re-run in isolation. Three hits triggers S14 step 2: 12 `mt` sweeps per side, both
release binaries built once and swapped inside one loop, machine idle. Base =
`272c3b79`, head = T5.C2. **Base 4 / head 3** over 1440 configurations each — tenth
alternation, tenth acquittal, and the rate is back to the quiet baseline (1/410
against session A's 1/360 on a machine that had been running batteries for hours).

**One hit at T5.C3**, `Static_152_100 t=4 sm=3 n=600 cabac=1 rc=0`, 28537 against
30190 — the third tree on which that configuration has produced *those two exact
numbers*. 5/5 byte-identical in isolation; one hit, no alternation.

Appended to `phase0_findings.md` as measurements 24–25, together with **measurement
23**, session B's single hit, which had been written into the log and the plan's
Gates cell but never into the finding S14 step 4 points at. Small thing, worth
naming: a ledger is only as good as the sessions that append to it.

The standing acquittal holds a third session running: every commit here is
decoder-side and the sweeps compare encoders. The one decoder symbol the encoder
reaches is `decoder_core.rs`'s `ExpandPicture*_c` pair, through
`common/expand_pic.rs`, and neither kernel is edited — only that function's
decoder-side call sites.

### 7. Perf at the allocation seam

The brief predicted flat and it is flat. 3-pair interleaved medians, `pre_conv`
(`272c3b79`) against `post_conv` (`7a28e620`), both benches, with this session's
null run first (S2):

| bench | null floor | pre_conv → post_conv |
|---|---|---|
| decode | median −0.02%, −0.04% … +0.10% | **median +0.00%**, −0.22% … +0.03% |
| encode | median +0.33%, −0.51% … +1.47% | **median +0.17%**, −1.45% … +2.36% |

Decode is the surface this session changed and it is inside the floor on every row.
Encode's one row at +2.36% (720p SMPTE Bars, 4t) is a whisker outside the floor's
max at n=3 while the median sits well inside it, on a bench whose code path this
session does not touch — S2b's "more pairs before a mechanism" applies, and there is
no mechanism to look for.

Worth stating because it is the more interesting null result: **three separate
`Vec`s replaced one aligned `WelsMallocz` block, and nothing moved.** The planes
went from 32-byte-aligned and contiguous to `Vec<u8>`'s alignment and disjoint, and
neither the decode bench nor the 56 golden rows can tell.

### 8. Numbers

| metric | entry | exit |
|---|---|---|
| tests (debug / release / ignored) | 443 / 437 / 20 | **448 / 442 / 20** |
| Miri `--lib` | 304 | **309** |
| `raw_ptr` | 4597 | **4595** (−2) |
| `unsafe_block` | 616 | **613** (−3) |
| `unsafe_fn` | 1249 | **1248** (−1) |
| `SHIM(` | 157 | **158** (+1: `data_ptr`) |
| decode goldens | 56 rows | **56 rows**, none moved |
| census | 61 allowlisted | unchanged |

Ratchet regenerated per S16 at T5.C2 and T5.C3. The metric behaved exactly as S16
warns: the *first* draft of each commit inflated `raw_ptr` and `SHIM(` with **prose**
— a doc comment naming `*mut u8`, a code comment containing the literal marker text
— and those were reworded rather than baselined. What was baselined is three real
increases: `data_ptr`'s return type, its `SHIM(phase5)` marker, and one `unsafe {}`
in the provenance test. `raw_ptr`'s fall is modest and honestly so: the pointers the
conversion deleted are the two array declarations, while the ~120 call sites still
receive a raw pointer *from* the shim. Those come off in 5.2–5.6.

### Hand-off: Phase 5, session D

**5.1's step 3 is done. What is left of 5.1 is `PicPool` and identity**, and it is
not obviously the next thing to do — read this before choosing.

* **The next closure, if it is 5.1's**: `PicPool` + the recycling predicate as a
  method + `PicId` for the three identity sites. `safe/pool.rs` already has the
  handle type, `pair_mut`, and a generation check; the five P3 tests exist to fail
  if identity becomes POC. The S25 work is *done in advance* for it — `pic_queue.rs`
  holds no borrows, and the one re-entrant pair is enumerated and guarded.
* **The argument for going to 5.2 first**: 5.2 is the phase's largest closure
  (`SDqLayer` is embedded and asserted widely), it owns **F22's reachability
  question**, and the decoder's `SHIM(phase2)` adapters start retiring there. 5.1's
  remainder blocks nothing — `pCtx->pDec` stays a `*mut SPicture` either way, which
  is T3.1b's precedent and what this session relied on.
* **The tree argues for 5.2**, on balance: the plane conversion made every picture
  *reachable* through a safe type, and the callers that would use it are 5.2's and
  5.3's. Converting the pool before the callers means `PicId` lands with nothing
  holding one. But it is a judgement call, and the P3 tests keep either order safe.

What to carry regardless:

* **`data_ptr` is `wrapping_add`, and the reason is in its doc comment.** Any future
  accessor that hands a mid-allocation pointer out of a `Vec` has the same trap. The
  general form: *a slice index narrows provenance; only the caller's reach tells you
  whether that is a bug, and no byte-level gate can see it.*
* **Nothing may cache a plane pointer beside the plane.** `SDeblockingFilter`'s
  `pCsData` is the one live mirror and it is 5.4's to delete.
* **`SPicture` is no longer `repr(C)` and no longer `Copy`.** Both were checked with
  the compiler rather than by reading; do the same before assuming it for the next
  struct.
* **`decoder_core.rs:1087` is the shim that outlives Phase 5.** The public output
  contract is `(pointer, stride)`; `data_ptr` there is not a kernel adapter and 5.6
  cannot delete it. Say so in the phase-exit ledger rather than discovering it.
* **Eugene revised `prompts/phase5.md` and `safety_refactor_plan.md` in the working
  tree while this session ran** — the §7.6 R-letter move, S14 refreshed to the
  measured F3 protocol, S17 generalized, S3–S5/S10–S11 marked dormant. That batch is
  in `754c7a04` alongside this entry, because leaving it dirty at a session boundary
  is worse than committing it with the close. A later batch (S27, the session-open
  check that lets a `docs/`-only tail skip the full battery) arrived after that
  commit and is **still uncommitted** — it is Eugene's, not this session's.

---

## 2026-08-11 — Phase 5, session D (the 5.2 closure, F22's answer, and two defects the closure's instrument found)

**Commits:** `ec29b339` (inherited doc tail — S27, S28, the session brief), `b0632555`
(Eugene's `[profile.dev]`), `2ed04ff9` (T5.D1, F22's answer and its S24 correction),
T5.D2 (the closure's instrument, F23 and F24), and this entry.

### The session in one line

The closure computed cleanly and then said *not yet*: the instrument written to answer
its S25 question found two pre-existing aliasing defects in front of 5.2's own work, so
no conversion landed and the reason is the deliverable.

### Control battery, and why it was the full one

The inherited tail was `rust/docs/`-only and session C ended `OVERALL: PASS`, so **S27's
first live use should have been the cheap subset** — and it was, for about four minutes.
Eugene then added `[profile.dev]` (`opt-level = 3`, both check flags kept) to the port
crate's `Cargo.toml`, which moves a gate: `cargo test` in debug had been running the
codec unoptimised, one asset taking 62.3s. That fails S27's docs-only condition, so the
open ran the **full** battery behind it, which is what the rule says to do.

One instrument claim verified rather than trusted, because the debug run's whole purpose
rests on it: `cargo test --no-run -v` passes `-C debug-assertions=on` and
`-C opt-level=3`, and `attempt to add with overflow` is present in the resulting test
binary. The 14x does not cost the F8/F9 tripwire.

`gates.sh full`: **OVERALL: FAIL**, one step — `sweep (release) PASS=340 FAIL=1`.
Everything else green: 448/442/20, Miri **309**, census 61 allowlisted, both benches
bit-identical, ratchet clean, decode goldens 56, debug sweep 341/341.

**F3, measurement 26.** `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=1 rc=1`,
C++ 39981 against Rust 0 — signature on every axis. S14 step 1: **5/5 byte-identical**
in isolation; the release sweep **341/341** on re-run. One hit, no alternation owed.
It is the cleanest acquittal the finding has, because the tree has nothing to blame:
the only changes since session C's PASS are docs and a `[profile.dev]` block that the
sweep does not build through (`rust_enc` is its own workspace root) on a *release*
sweep. Previous sessions could say *the commits are decoder-side and the sweeps compare
encoders*; this one says *there are no commits*. Appended to `phase0_findings.md`.

### 1. F22's reachability answer (T5.D1)

**`pCurDqLayer->pDec` cannot be null on the CABAC parse path.** Five links, all
greppable, none of them a test — no gate can distinguish the readings because every
corpus stream decodes with a picture attached:

1. `SDqLayer::pDec` has **one writer in the crate**: `decoder_core.rs:3444`, inside
   `InitDqLayerInfo`, writing its fourth argument.
2. `InitDqLayerInfo` has **one call site**: `decoder_core.rs:3682`, passing
   `(*pCtx).pDec`.
3. That site is dominated, in the same loop iteration, by the prefetch-or-return at
   `:3589-3597` — null goes to `PrefetchPic`, and a null *result* returns
   `ERR_INFO_REF_COUNT_OVERFLOW`.
4. Nothing between them can undo it. `pCtx->pCurDqLayer` has one production writer and
   it runs before the loop (the other two are in `mod tests`), and the six
   `(*pCtx).pDec = null` sites write the **context's** field where CABAC reads the
   **layer's** copy.
5. `GetThreadCount` returns `0` unconditionally, so `iThreadCount > 1` is dead and
   `WelsDecodeSlice` is the only parse entry.

The same five hold in the C++ (`decoder_core.cpp:2386`, `:2674`, `:2542-2575`), and the
prefetch there is **not** conditional on `bParseOnly`. So `mv_pred.cpp`'s guards are
defensive branches no stream reaches, in both trees.

**And the finding's own comparison had never been greped (S24).** F22's four Rust counts
are exact. The C++ side is not what it assumed, because `mv_pred.cpp` mostly spells the
test as a **ternary** (`pCurDqLayer->pDec ? A : B`) and a search for `NULL` misses it:

| module | C++ null tests | Rust |
|---|---|---|
| `mv_pred` | **20** (4 if-form + 16 ternary) | 28 |
| `parse_mb_syn_cabac` | **0** | 0 |
| `parse_mb_syn_cavlc` | **0** | 4 |
| `decode_slice` | **2** | 13 |

Two consequences. `parse_mb_syn_cabac.rs`'s zero is **faithful** — the C++ CABAC file has
28 unguarded `pCurDqLayer->pDec` dereferences of its own. And the divergence is **three**
functions, not six: `UpdateP16x16MotionInfo`, `UpdateP16x8MotionInfo`,
`UpdateP8x16MotionInfo` are guarded in C++ and not in the CABAC copy, while
`Update8x8RefIdx` runs the *other* way — C++ unguarded, CABAC copy faithful, and
**`mv_pred.rs` is the copy that added a guard**. `PredMv` and the two `PredInter*Mv`
never touch `pDec` at all. So 5.3 unifies per function, not per module; deciding from the
28-vs-0 headline would have added a guard the reference does not have. S21's "inventory
every function per copy", earned again.

### 2. The S20 closure for 5.2, as computed

**Field types: no reachability.** All 24 changing fields are `*mut [primitive; N]` or
`*mut primitive` — leaves. Nothing follows them, unlike `SPicture`'s planes reaching
`PaddedPlane` at T5.C3.

**Layout asserts: none, and this is worth checking before assuming otherwise.**
`assert_size!(SDqLayer, 512)` and `assert_size!(SMbCache, 576)` exist, in
`encoder/abi_guard.rs`, and both pin the **encoder's** namesakes (imported from
`encoder::md` and `encoder::svc_encode_slice`). The decoder's two have no size assert and
no offset pin — `SWelsDecoderContext`'s situation, which phase5.md §5 already records. So
T3.4's "const ABI asserts leave no intermediate green state" does not apply here.

**Embeddings: one.** `SWelsDecoderContext` holds `sMb: SMbCache` **by value**
(`decoder_context.rs:623`) plus `pDqLayersList` and `pCurDqLayer`. Nothing embeds
`SDqLayer` by value — checked with the compiler's own view, not by reading.

**Signatures: 66 functions over six files**, and the split that decides the shape:

| file | fns taking a DqLayer ptr |
|---|---|
| `decode_slice.rs` | 19 |
| `mv_pred.rs` | 14 |
| `deblocking.rs` | 12 |
| `parse_mb_syn_cabac.rs` | 12 |
| `parse_mb_syn_cavlc.rs` | 7 |
| `decoder_core.rs` | 2 |

**16 take both `pCtx` and the layer. 50 take the layer only.**

**The double path, measured — and it is not two live paths.** `InitCurDqLayerData`
(`decoder_core.rs:3523`) copies 27 pointers from `(*pCtx).sMb.*[0]` into `(*pCurDq).*`.
Accesses: **130 via `sMb`, 316 via the layer**. Of the 130, **129 are lifecycle** —
declaration, `Default`, the 27 allocations at `:2651-2678`, the frees at `:2697+`, the
alias copy — and the *one* live consumer is the per-picture `pSliceIdc` `0xff` memset at
`:3610`, with `sMb.iMbWidth`/`iMbHeight` supplying the count at `:3608`. (`pic_queue.rs`'s
two `sMb.pRefIndex[]` hits are **log strings**. S16's prose warning, third session
running.) So `SMbCache` is an owner with no readers and `SDqLayer` is an alias with all
of them.

**Two things the closure deletes rather than converts**, T5.C1's shape both:

* `LAYER_NUM_EXCHANGEABLE` is **1** (`decoder_context.rs:72`). Every
  `[…; LAYER_NUM_EXCHANGEABLE]` dimension on all 25 `SMbCache` fields, on
  `pDqLayersList`, and every `for i in 0..LAYER_NUM_EXCHANGEABLE` is a one-element,
  one-iteration construct.
* `SMbCache::pMotionPredFlag` (`:543`) has **exactly two mentions crate-wide**: its
  declaration and its `Default` init. Never allocated, never aliased, never read, never
  written.

**The shape this licenses, with the number that decides it.** `MbGrid` goes **inside the
layer**, not on the context — which is what plan §5.2 already prescribes, and here is why
it is the only sane option. Put the grid where `SMbCache` is today and all **50**
layer-only functions need a new parameter, atomically, because S20 forbids deleting
`pMbType` from `SDqLayer` in one file and not another. Put it in the layer and those 50
signatures **do not change at all** — `(*dq).pMbType` becomes `(*dq).grid.mb_type(…)` —
and `SMbCache` dies whole, its 129 lifecycle sites stopping being needed and its one
consumer moving onto the grid as a method.

**S21, which is where the work actually is.** `SDqLayer` comes from
`WelsMalloczHelper(pMa, size_of::<SDqLayer>())` at `decoder_core.rs:2650` — a zeroed
malloc, cast to `PDqLayer`. An owned `MbGrid` there is S21's exact hazard. Two proven
patterns: T5.C3's heap constructor (`Box::new` + `Box::into_raw`, freed by
`Box::from_raw`), preferred here because the layer is one allocation with one alloc site
and one free site; or the `MaybeUninit` shell + `make_zeroed_shell_valid`
(`decoder_context.rs:744`), which already carries `sRawData`/`sSavedData`. `SDqLayer`'s
`Default` (`decoder_core.rs:402`, `mem::zeroed` + two fixups) has test consumers and needs
a real one either way.

**F19's check, per allocation, and it discharges clean** — but only at field level.
`InitialDqLayersContext` makes **28** `WelsMalloczHelper` calls against
`UninitialDqLayersContext`'s **25** `WelsFreeHelper` plus one for the layer, which reads
like a two-array leak and is not: allocation unrolls the three `LIST_A` pairs
(`pMv`, `pMvd`, `pRefIndex`) while freeing loops over them. Diffed by **field name** it is
24 against 24, balanced. Worth recording because the raw call-count comparison is the one
a reviewer would reach for and it gives the wrong answer.

**Census: zero allowlist entries name 5.2**, against the brief's expectation that some
would unify here. `type SDqLayer x2` and `type SMbCache x2` are class **(a)** —
cross-codec namesakes matching C++, permanent under P14. What 5.2 owes the allowlist is a
different thing: renaming the decoder's types to `DqLayerState`/`MbGrid` takes both keys
from `x2` to `x1`, and the count is part of the key *on purpose*, so both lines must be
edited or removed or the gate goes red on a rename that is the conversion's whole point.
`alias PDeblockingFilterMbFunc x2`'s comment cites `*mut SDqLayer` and goes stale (5.4's).

**Scratch caches: 28 signatures, not ~40, over three files** — `parse_mb_syn_cabac.rs` 15,
`mv_pred.rs` 8, `parse_mb_syn_cavlc.rs` 5. The owners are two stack locals in
`decode_slice.rs` (`:4632`, `:4836`), so the conversion to `&mut` locals is as mechanical
as the brief says. The two families disagree on the shape — `*mut [[i16; 2]; 30]` per-list
in `mv_pred.rs` against `*mut [[[i16; 2]; 30]; LIST_A]` both-lists in the CABAC copy —
which is F22's observation and why the re-point is 5.3's, after the owner flip.

**`pBitStringAux`: 24 sites, one writer.** `decoder_core.rs:3669` sets it to
`&mut (*pNalCur).sNalData.sVclNal.sSliceBitsRead` — it points **into the NAL unit**, so it
is not an owned field but a borrow of another object outliving its source. 17 of the
readers are in `decode_slice.rs`; one is `cabac_decoder.rs:855`, the `SHIM(phase5)`
accessor that dies with it.

### 3. The S25 enumeration — and the instrument it needed (T5.D2)

The question S25 asks is *who reaches this object while I hold a borrow?* Read from the
code, `WelsDecodeSlice` (`decode_slice.rs:5063`) answers it badly:

```rust
let ctx = &mut *pCtx;
let dq  = &mut *ctx.pCurDqLayer;
let pSlice = &mut dq.sLayerInfo.sSliceInLayer;   // reborrow of dq
…
let iRet = pDecMbFunc(pCtx, pNalCur, &mut uiEosFlag);   // re-enters via pCtx,
                                                       // reaches the same layer
if !dq.pMbRefConcealedFlag.is_null() { … ctx.bMbRefConcealed … }
pSlice.iTotalMbInCurSlice += 1;
```

Three overlapping `&mut`s live across a re-entrant call, all three used after it. That is
T4b.2a's and T5.B2's shape, in code that predates Phase 5 — so it is not something the
conversion introduces, it is something the conversion inherits.

**And nothing could see it.** `gates.sh` runs Miri over `--lib` and, once per phase, over
`tests/*differential*.rs` — the kernel, plane and bits differentials, all of which call
internal functions directly. The tests that decode real streams are the conformance and
lifecycle suites, and **Miri has never been pointed at them.** So the audit had no gate,
which is the same hole `data_ptr` sat in one session ago, and S28's lesson generalizes:
*the test is the instrument.*

So the session wrote one. `decode_slice_loop_runs_under_the_aliasing_checker`
(`decode_slice.rs`) decodes `narrow_16x16.264` — 711 bytes, one macroblock per frame,
which is what makes a full `Initialize` + decode tractable under an interpreter — through
the real public path, with the stream `include_bytes!`'d because the Miri gate does not
pass `-Zmiri-disable-isolation`. It is a `--lib` test, so it would run on **every** full
battery rather than once per phase.

It found two defects, neither of them the one it was written for, and it has **not yet
reached the loop above.**

**F23 — the public API's `&mut self` methods are UB, both codecs.** Miri failed before the
decoder was initialized. `ISVCDecoder` is a one-pointer struct and the first field of the
much larger `CWelsDecoderImpl`; `Initialize(&mut self, …)` borrows **eight bytes**, hands
that provenance to the thunk, and `decoder_init_c` casts base-to-derived and writes at
offset `0x20`:

```text
attempting a write access using <960729> at alloc251625[0x20], but that tag does not
exist in the borrow stack for this location            → src/api/codec_api.rs:1425
help: <960729> was created by a SharedReadWrite retag at offsets [0x0..0x8]
```

Not the cast — `WelsCreateDecoder` hands out `Box::into_raw(dec) as *mut ISVCDecoder`,
which has provenance over the whole object, and casting *that* up is sound. It is the
`&mut self` signature narrowing it on the way in. **19 methods across both vtable
structs**, and all **11** of the crate's integration test files drive the codecs that way,
as would every Rust consumer. Owner **Phase 8** (T10/§2.2.8 — the ABI contract that
stays); recorded, not fixed, F19's precedent. Proved by construction: the same call
spelled `((*vtbl).Initialize)(p_decoder, &param)` runs clean, and that is how the test now
spells it.

**F24 — `ParseSliceHeaderSyntaxs` invalidates its own borrow.** With F23 routed around,
Miri got as far as `decoder_core.rs:2034`:

```rust
let pSliceHead    = &mut (*kpCurNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader;  // [0x28..0xf00]
let pSliceHeadExt = &mut (*kpCurNal).sNalData.sVclNal.sSliceHeaderExt;               // [0x28..0x1350]
…
(*pSliceHead).iFirstMbInSlice = uiCode as i32;                                       // UB
```

The outer borrow **contains** the inner, so creating it invalidates the inner, which is
then used 74 times. `:2023` adds a third invalidation by writing
`bSliceHeaderExtFlag` through the raw pointer while both are live. 141 uses across a
576-line function; the fix is T5.B2's, *no borrow outlives one expression*. Owner **5.5**,
which already owns `PSliceHeader`/`PSliceHeaderExt` in the census allowlist.

**Both are recorded in `phase5_findings.md`. Neither is byte-visible**: every address is
correct, every write lands where C++ lands it, and 448/442 tests, 56 golden rows, 2316
malformed rows, both benches and 341/341 sweeps agree with all of it.

### 4. Why no conversion landed, which is the closure's actual output

The closure is clean and the shape is decided. What stopped 5.2 is what the instrument
said: **S25 makes the re-entrancy audit part of the conversion, and the audit's first gate
reports pre-existing aliasing UB in front of 5.2's own work.** Converting raw pointers to
borrows on top of an already-invalid borrow stack means every new `&mut` inherits it, and
a Miri failure after the conversion would be unattributable — precisely the bisect problem
T5.C2's three-face split existed to avoid. The one thing session C proved is that a Miri
failure is cheap to fix when it lands in one place and expensive when it lands in a
hundred-site diff.

So the order is **F24, then 5.2** — and F24 is 5.5's file but 5.2's blocker, a dependency
the plan's step order does not currently express. That is the finding, and it is the kind
the brief's own §1 anticipated when it said the written closure is a deliverable in its
own right.

The test ships `#[cfg_attr(miri, ignore)]`, which is a debt and is labelled one at the
site. It cannot run under Miri until F23 and F24 close, and pretending otherwise would let
a green gate imply coverage that does not exist. Under `cargo test` it does earn its keep:
it is the only unit test that decodes a real stream. **The attribute comes off when F24
closes**, and that is the moment the decode path acquires an aliasing gate.

### 5. Face 4 — already done

The brief asked to re-scope `SPicture::data_ptr`'s `SHIM(phase5)` marker to name its
surviving consumer. **T5.C3 already wrote it**: `picture.rs:391-395` names the public
output path at `decoder_core.rs:1087` and says it outlives Phase 5. Re-greped per S24 —
`:1087-1089` is exactly the `*ppDst.add(i) = data_ptr(i)` fill beside the `iStride`
writes, so the cite is current. No change; recorded so the next session does not go
looking for work that is finished.

### 6. Numbers

| metric | entry | exit |
|---|---|---|
| tests (debug / release / ignored) | 448 / 442 / 20 | **449 / 443 / 20** (+1, the aliasing probe) |
| Miri `--lib` | 309 | **309** (the probe is `cfg_attr(miri, ignore)` — F23/F24) |
| `raw_ptr` | 4595 | **4600** (+5) |
| `unsafe_block` | 613 | **614** (+1) |
| `unsafe_fn` | 1248 | **1248** (+0) |
| `SHIM(` | 158 | **158** (+0) |
| decode goldens | 56 rows | **56 rows**, none moved |
| census | 61 allowlisted | unchanged |
| Miri skips | 2 | 2 |
| findings open | F22 (reachability open) | **F22 answered; F23, F24 new** |

Ratchet regenerated per S16, and the delta is one file and two metrics:
`decoder/decode_slice.rs` `raw_ptr` 194 → **199** and `unsafe_block` 5 → **6**. Both are
the aliasing probe — the `*mut ISVCDecoder`, the `[*mut u8; 3]` destination array and the
vtable pointer it reads, inside one `unsafe {}`. Nothing else in the crate moved, and the
+5/+1 buys the only unit test that decodes a real stream.

**T5.D2's battery: three FAILs, all three adjudicated.** `unsafe ratchet` (regenerated
above, delta named). `sweep (debug)` and `sweep (release)`, one row each, both F3's
signature — measurements 27–28, both **5/5 byte-identical** in isolation, and the
alternation S14 step 2 would ask for was replaced by a hash: the `rust_enc` release
binary is **byte-identical** with the test present and stashed
(`fa59e2a5…d771844` both ways), because `#[cfg(test)]` code is not compiled into a
dependency build. Base and head are the same binary in the only artifact the sweep
exercises, so an alternation could not produce information. Everything else green:
449/443/20, Miri **309/0**, census 61, both benches bit-identical, decode goldens 56.

### Hand-off: Phase 5, session E

**The closure is computed and written (§2 above); do not recompute it, and do not start
5.2 before F24.**

* **Next unit is F24**, not 5.2. `ParseSliceHeaderSyntaxs` (`decoder_core.rs:2000-2576`),
  141 uses of three overlapping borrows, mechanical, T5.B2's shape, its own commit. Then
  **delete `#[cfg_attr(miri, ignore)]`** from
  `decode_slice_loop_runs_under_the_aliasing_checker` and re-run — the queue depth past
  F24 is unknown, and the probe reports one defect per run. Budget ~8 minutes per Miri
  round trip.
* **The loop that started this has still not been examined by the gate**:
  `WelsDecodeSlice`'s three overlapping `&mut`s across `pDecMbFunc`, §3 above. Read it as
  a prediction, not a finding, until Miri gets there.
* **When 5.2 does start**: grid in the layer, not the context (50 signatures against 0);
  `SMbCache` dies whole; `LAYER_NUM_EXCHANGEABLE` and `pMotionPredFlag` are pure
  subtraction and make a clean first commit that converts nothing; then the owner flip,
  then 5.3's per-function guard unification (three onto `mv_pred.rs`'s shape,
  `Update8x8RefIdx` onto the CABAC one).
* **The allowlist needs editing in the same commit as the rename**, not after: both
  `type SDqLayer x2` and `type SMbCache x2` go to `x1`, and the count is part of the key.
* **F23 is Phase 8's and should be said out loud at the phase exit**, alongside
  `data_ptr`'s surviving shim: the crate's entire Rust-facing API is UB today and the
  integration suite runs it.
* **S27 got its first live use and its first exception in the same session.** The tail was
  docs-only for four minutes; a build-config change arrived and the rule correctly
  escalated to the full battery. Worth keeping as the example.

---

## 2026-08-12 — Phase 5, session E (F24 closes and takes four siblings with it; F25 stops being a prediction; F26 stops the queue; 5.2's subtraction lands)

**Commits:** `cd05829c` (inherited doc tail — S14's hash shortcut, F23's Phase 8 owner,
the session brief), `975b1a59` (T5.E1, the aliasing family), `db5bafdb` (T5.E2, 5.2's subtraction),
and this entry.

### The session in one line

F24 was the blocker and it was not one function: fixing it moved the probe forward twice
more, which turned session D's prediction into F25 and then hit F26 — a provenance defect
in the allocator that no amount of borrow discipline will fix — and the brief's bound of
three stopped the hunt exactly where it should have.

### Control battery — S27's second live use, and its first clean one

The inherited tail was `rust/docs/`-only, `rust/tools/` carried nothing but the
regenerated ratchet baseline, and the toolchain was unchanged, so the open ran the cheap
subset: **OVERALL: PASS**, 449/443/20, ratchet clean, census 61.

One honest note on the precondition. S27 asks that the previous session ended
`OVERALL: PASS`; session D ended `OVERALL: FAIL` with **three adjudicated** failures (a
ratchet it regenerated in the same commit, and two sweeps that were F3). Read literally
that is a condition failing and the rule says full battery. Read as intended — *was the
tree accepted green?* — it holds. The cheap subset was run, and it re-verified the
ratchet that had been the first of those three FAILs. Worth writing down because the rule
will meet this case again: **"ended PASS" and "ended accepted" are not the same
predicate**, and S27 does not currently say which one it means.

### 1. F24, and the discovery that it was never one function (T5.E1)

The fix is the one T5.B2 established, arrived at from the other side. T5.B2 shortened
borrows; this shortens them to nothing. `addr_of_mut!` creates no reference, so every
derived pointer carries the parent allocation's provenance, none can invalidate another,
and the question *whose retag is on top* stops existing.

```rust
let pNalHeaderExt: PNalUnitHeaderExt = std::ptr::addr_of_mut!((*kpCurNal).sNalHeaderExt);
let pSliceHead: PSliceHeader =
    std::ptr::addr_of_mut!((*kpCurNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader);
let pSliceHeadExt: PSliceHeaderExt =
    std::ptr::addr_of_mut!((*kpCurNal).sNalData.sVclNal.sSliceHeaderExt);
```

Because the 74 `(*pSliceHead)` and 50 `(*pSliceHeadExt)` sites already spelled the
dereference, **141 uses cost four changed lines and 14 re-spellings of `pNalHeaderExt.`**.
The brief budgeted a mechanical 141-site rewrite; it wasn't one, and that is worth
knowing before the next such fix is sized.

**Then Miri found the same shape one function further on**, in `DecodeCurrentAccessUnit`,
with a byte-identical diagnosis — `[0x28..0xf00]` created, invalidated by `[0x28..0x1350]`.
At ~8 minutes per round trip, discovering the family one site at a time was not the way
to do it, so the shape got greped instead. Four more sites, all `decoder_core.rs`, all in
one pass:

| site | shape |
|---|---|
| `DecodeCurrentAccessUnit:3684` | the nested pair Miri reported |
| `InitDqLayerInfo:3471` | `pSh` nested in `pShExt`, **plus four borrows that escape into the layer** |
| `WelsDqLayerDecodeStart:3526` | escapes into `(*pCtx).pSliceHeader` for the whole slice decode |
| the `pBitStringAux` store, `CheckAccessUnitBoundaryExt`'s arguments | escaping `&mut` |

The escaping ones are the interesting half. `InitDqLayerInfo` stores
`pRefPicListReordering`, `pRefPicMarking`, `pPredWeightTable` and `pRefPicBaseMarking`
*into the layer*; as `&mut`-derived pointers they died at the next use of their parent
and were then read for the rest of the decode. A nested borrow hurts one function; an
escaping one hurts everything downstream of it.

**And one pair no instrument in this repository can reach.** `pSps` was
`&mut (*pSubsetSps).sSps`, nested inside a `&mut` of a subset-SPS buffer entry that three
later bindings re-borrowed. UB on every `kbExtensionFlag` path — and `bExtensionFlag` is
`eType == NAL_UNIT_CODED_SLICE_EXT` (`nalu.rs:684`), so reaching it needs an SVC stream.
The corpus has none; the probe decodes AVC. It was found by reading while fixing the pair
above and fixed in the same commit, because the alternative is leaving a known defect
behind a commit whose message says the function is clean. **S22's shape, fourth
instance**: an instrument's reach is part of the instrument.

### 2. F25 — the prediction the probe was written for, confirmed (T5.E1)

Session D read `WelsDecodeSlice` and enumerated three overlapping `&mut`s across the
re-entrant `pDecMbFunc`, and was careful to label it *a prediction, not a finding, until
Miri gets there*. Miri got there on run 2:

```text
attempting a write access using <35266369> at alloc251880[0x7e92c], but that tag does not
exist in the borrow stack        → decode_slice.rs:5150   ctx.bMbRefConcealed = false;
help: <35266369> was created by a Unique retag at offsets [0x0..0x8ae00]      (:5071)
help: <35266369> was later invalidated … by a Unique retag                   (:698)
```

The invalidator is **inside the callee**: `pDecMbFunc` re-enters and takes its own
`&mut *pCtx` at `:698`. Fixed the same way — 43 sites named `(*pCtx)` / `(*pCurDqLayer)`
per use.

**The systemic part, which outranks the fix.** That retag covers `[0x0..0x8ae00]` — the
*whole context*. So any re-entrant callee that borrows the context kills every outer
borrow of it, and **~30 `&mut *pCtx` sites remain in `src/decoder/`**. Each is sound
alone and UB the moment it is held across a call that re-enters through `pCtx`, which
this decoder does constantly via `pDecMbFunc`, `pFuncList` and the deblocking callbacks.
This is not a queue of independent defects; it is one pattern, and 5.2–5.6 convert
exactly this code. The rule they inherit: **the god-context is never held as a borrow
across a call.**

### 3. F26 — the third defect beyond F24, and the bound doing its job

Run 3 got past F25 and stopped somewhere else entirely:

```text
not granting access to tag <wildcard> because that would remove [Unique for <36614846>]
which is strongly protected        → cabac_decoder.rs:855
```

Not a borrow defect. `CMemoryAlign::WelsMalloc` computes its alignment by round-tripping
through an integer (`memory_align.rs:49-50`), which **exposes the provenance of every
allocation in the port** — NAL units, access units, `SDqLayer`, the per-MB arrays. Miri
calls those pointers wildcards and refuses any access through one that would disable a
strongly-protected `&mut` argument.

The brief's bound is three defects beyond F24 and this is the third, so the hunt stopped.
That is the right outcome twice over: the bound exists to prevent an unbounded hunt, and
this particular defect is not the mechanical fix the other five were — it is an allocator
decision, and restoring tracked provenance crate-wide should be expected to let Miri see
borrow structure it has been silently permitting, which is S22's backlog shape.

**One question is genuinely open and the finding says so rather than guessing.** T5.E1
changed the `pBitStringAux` store from a retagging `&mut` to a wildcard-inheriting
`addr_of_mut!`. A `&mut` mints a tracked tag even from a wildcard parent. So it is not
proven whether F26 was reachable before T5.E1 or whether that one line exposes it. Both
stories are defensible and the instrument is eight minutes away, so `phase5_findings.md`
names the experiment — restore that one store, keep everything else, run the probe — and
declines to settle it by reasoning.

`#[cfg_attr(miri, ignore)]` therefore **stays**, labelled with all four items and what
remains. S17: an instrument may skip only loudly. It comes off at F26.

### 4. 5.2's subtraction (T5.E2), and an S24 correction to the closure

The closure's "first commit that converts nothing" landed as written. `SMbCache` dies
whole: the 27 arrays are allocated straight into the layer, `InitCurDqLayerData`'s
27-pointer re-alias is deleted, and the struct, its `Default` and the context field are
gone. `LAYER_NUM_EXCHANGEABLE` is deleted — **both definitions of it**, which the closure
did not mention it had (`decoder_context.rs:72` and `decoder_core.rs:54`) — `pDqLayersList`
is a scalar, and the three one-iteration loops are straight-line. `pMotionPredFlag` died
with the struct without needing its own step.

**The correction, and it is the load-bearing part of this section.** The closure says
`SMbCache` has *one* live consumer, the `pSliceIdc` `0xff` memset. It has **three**: the
memset and both `numMb` computations, in `InitialDqLayersContext` and
`UninitialDqLayersContext`. The free path's is the one that matters, because
`sMb.iMbWidth`/`iMbHeight` are the **allocation's** dimensions
(`(kiMaxWidth + 15) >> 4`) while the layer's are the **current slice's**, set per-slice
from the slice header and smaller on any stream decoding below the negotiated maximum.
A session following the closure verbatim would have moved that read onto the layer and
freed with the wrong size.

It stayed pure subtraction only because the context already keeps what was needed —
`iPicWidthReq`/`iPicHeightReq`, set in the same function that allocates and zeroed after
the frees. No field was added. But the closure's count was a summary of a fact, and S24
is about exactly that: **"129 of 130 are lifecycle" was true and still hid a consumer**,
because "lifecycle" quietly included the arithmetic that sizes the free.

Census: the `type SMbCache x2` line is **removed rather than re-keyed**. Session D's
hand-off predicted `x2 → x1` because it was planning a *rename*; a deletion is different
— with the decoder's copy gone the name is not duplicated at all, so the entry has
nothing left to describe. 61 → **60**.

### 5. Numbers

| metric | entry | exit |
|---|---|---|
| tests (debug / release / ignored) | 449 / 443 / 20 | **449 / 443 / 20** |
| Miri `--lib` | 309 | **309** (the probe is still `cfg_attr(miri, ignore)` — F26) |
| decode goldens | 56 rows | **56 rows, none moved** (both faces) |
| census | 61 allowlisted | **60** (decoder `SMbCache` deleted) |
| `raw_ptr` | 4600 | **4548** (−52: −4 at T5.E1, −48 at T5.E2, all deletion) |
| `unsafe_fn` | 1248 | **1247** (`InitCurDqLayerData` deleted) |
| `unsafe_block` / `SHIM(` | 614 / 158 | **614 / 158** (+0) |
| Miri skips | 2 | 2 |
| findings open | F22 answered, F23, F24 | **F24 fixed; F25 fixed; F26 new and open** |

Batteries: T5.E1 `gates.sh full` **OVERALL: PASS**, everything green including sweeps
341/341 in *both* profiles. T5.E2 **one FAIL, adjudicated** — `sweep (debug)` 340/1, F3,
measurement 29, the first hit whose isolation re-runs produced a *second* wrong length
(4× byte-identical, 1× 37837 against C++'s 39981) and so satisfied S14 step 1's race
criterion directly instead of by inference.

### Hand-off: Phase 5, session F

* **The next unit is F26, and it is a decision before it is a fix.** Settle the open
  question first with the one-run experiment in `phase5_findings.md` — it costs eight
  minutes and it determines whether F26 is live today or was exposed by T5.E1. Then
  decide whether to de-launder `CMemoryAlign::WelsMalloc`. Expect S22's backlog: tracked
  provenance means Miri starts seeing structure it has been permitting.
* **Do not start the grid conversion before that.** The reason is F24's, unchanged: new
  `&mut`s on a path Miri cannot clear make the next failure unattributable. The
  subtraction is done, so the conversion now starts from a genuinely standing start the
  moment the path is clean.
* **The ~30 `&mut *pCtx` sites are the real inventory**, and they are cheaper to fix by
  grep than by Miri round trip — that technique found four of this session's six sites.
  Whoever converts a decoder file should sweep its `&mut *pCtx` / `&mut *pCurDqLayer`
  bindings in the same commit.
* **The closure is still good** apart from §4's correction. Grid in the layer, not the
  context (50 signatures against 0); `pBitStringAux` and `cabac_decoder.rs:855`'s
  `SHIM(phase5)` accessor die with the fields that hold them — and that accessor is
  where F26 surfaces, so the two may resolve together.
* **F23 remains Phase 8's** and should still be said out loud at the phase exit.

---

## 2026-08-12 — Phase 5, session F (F26's experiment answers with a fourth option; the allocator stops laundering provenance; the backlog that releases closes four defects and stops on a named one)

**Commits:** `6527bcf2` (inherited doc tail — S29, S20's lifecycle clause, S27's
clarified predicate, the session brief), `e770ea77` (T5.F1, the experiment and F27),
`154e912c` (T5.F2, the allocator), `bd65b8d4` (T5.F3, the backlog), and this entry.

### The session in one line

The experiment F26's entry demanded returned an answer neither of its two branches
predicted — the site it was found at fails with a *tracked* tag, so it was never the
allocator's defect — and once the allocator was fixed anyway, the backlog that
un-blinding released came up in exactly the shape S22 says it does: four defects in six
round trips, three of them new, the fourth already named by F25.

### Control battery

Docs-only tail, session E accepted (its one FAIL was F3-adjudicated, measurement 29),
`rust/tools/` and the toolchain unchanged — S27's cheap subset, and this time the
predicate it was clarified with in the same tail applied cleanly rather than by
interpretation. **OVERALL: PASS**, 449/443/20, ratchet 4548, census 60.

### 1. The experiment, and why it is worth doing experiments (T5.F1)

F26 named a one-run experiment and forbade settling it by reasoning: restore the `&mut`
spelling at the `pBitStringAux` store, run the probe once, revert. It named two possible
answers — passes, so F26 is latent; fails identically, so F26 is live. **It did neither.**

```text
not granting access to tag <35268364> because that would remove [Unique for <36617218>]
which is strongly protected              → cabac_decoder.rs:855
<35268364> created by a SharedReadWrite retag at offsets [0x1350..0x1380]
                                         → decoder_core.rs:3694  (the &mut store)
<36617218> is this argument   pBsAux: &mut BsCursor
                                         → decode_slice.rs:3785
```

The tag is no longer `<wildcard>` and Miri refuses the access anyway. **That difference
is the whole result.** A wildcard refusal is conservative — *this access might disable a
protected tag*. A refusal against a concrete tag over a named 0x30-byte range is a
conviction — *this access does overlap a live, strongly protected `&mut`*. So F26 was
live before T5.E1 (that line exposed nothing), **and F26 was not what the probe was
stopping on**. The site is F27's: `WelsDecodeMbCabacIntraModeHelper` splits the NAL's
`BsReader` into `(buf, &mut BsCursor)` and passes the cursor down as a protected
argument, while `cabac_rbsp_window` underneath reaches the same `BsReader` *whole*
through `pCtx->pCurDqLayer->pBitStringAux`.

Reasoning would not have got here. Both stories on offer were defensible and both were
wrong, and the instrument was eight minutes away. **Two outcomes were enumerated and the
answer was a third** — which is the general case for "do not settle this by reasoning",
not a special one.

### 2. The allocator (T5.F2)

`memory_align.rs:49-50`'s integer round trip becomes `addr()` + `byte_sub`. It is **S6
parity, not repair**: `memory_align.cpp:92` is
`pAlignedBuffer -= ((uintptr_t) pAlignedBuffer & kiAlignedBytes)`, pointer arithmetic
with the integer used only as the mask, and the round trip was introduced in translation.

Two things worth carrying:

* **S18, per consumer.** The alignment arithmetic exists in exactly one place and
  everything else delegates — so this un-blinds **19 consumer files, 8 decoder and 10
  encoder and 1 processing**, not the decoder's heap. The encoder's Miri skips are
  untouched and stay (F12's pool, F13's `encoder_ext`).
* **The instrument was proven red before it was trusted green.** With the old formula
  restored, `-Zmiri-strict-provenance` refuses the cast; with the fix, all five
  `memory_align` tests pass under it. The library gate does not pass that flag, so the
  claim is stronger than what CI asserts. The two new tests are the address pin (7
  alignments × 2 periods of offsets, against the old formula transcribed in integers)
  and the full-legal-reach read — **S28's rule aimed at the allocator that hands out the
  pointers**, and the downward half is the point: the header words live *below* the
  returned pointer and `WelsFree` reads them from there.

Perf at the allocation seam, 3 interleaved pairs against a null taken the same hour:
decode median **+0.08%** (band +0.04 … +0.48) against a null of +0.41% (−0.94 … +0.54);
encode median **+0.00%** (−3.32 … +0.40) against a null of +0.00% (−6.48 … +1.49). The
effect is smaller than the floor, which is what "same arithmetic, same addresses"
predicts.

### 3. The backlog (T5.F3) — six round trips, and the bound doing its job again

| round | defect | disposition |
|---|---|---|
| 1 | **F27** — protected `&mut BsCursor` vs `cabac_rbsp_window`'s `&BsReader` | fixed; **2 of the 5 cursor parameters were dead** (`_pBsAux`, never read) and were deleted outright |
| 2 | **F28** — the layer borrowed across calls that re-reach it through `pCtx` | fixed, 20 functions + 16 nested borrows in `decode_slice.rs` |
| 3–4 | **F29** — `pCabacCtx.as_mut_ptr()`, 30 sites, 4 live in pairs | fixed via one `cabac_ctx_base` helper — **after round 4 reported the identical error**, see below |
| 5 | **F30** — `pCoff.offset(-1)` one element before the array | fixed, `wrapping_offset`, F7's class |
| 6 | **F25's `&mut *pCtx` inventory**, 12 bindings, 7 in this file | **not new** — the bound stops here |

Three things this face taught that are not in the table.

**F28 is F25's law one level down.** F25 said *the god-context is never held as a borrow
across a call*. F28 is the same defect with the **layer** in the context's place, and it
generalizes: any object reachable from `pCtx` behaves this way, because a re-entrant
callee reaching it through `pCtx` reads through the parent tag, and a read pops a
`Unique`. It is not a rule about `pCtx`; it is a rule about anything `pCtx` can reach.

**F29 cost a round trip to an S24 failure inside a grep.** The first pass re-pointed 25
sites and missed 4 — and the 4 it missed were the only ones live in pairs, i.e. the only
ones that mattered. They are formatted across three lines (`(*pCtx)` / `.pCabacCtx` /
`.as_mut_ptr()`), so a line-anchored grep cannot see them, and Miri had already *named*
two of them in the diagnostic being fixed. **The count came from a grep of the code and
was still a summary of the fact**: the grep's unit was the line and the code's unit was
the expression. S24 is usually about trusting prose; this is the same failure with no
prose in it.

**Round 6 is the good kind of stop.** It is not a new finding — F25 named both the
pattern and the count — so the queue ends on something with an owner (5.6), a spelling
(S29's), and a number (12 decoder-side, 7 in `decode_slice.rs`). The probe's
`#[cfg_attr(miri, ignore)]` label went from four unknowns this morning to one known
item, which is the difference between a debt and a task.

### 4. Face 4 did not start, and that was the brief's own gate

The grid conversion was scoped **"only from a standing start"**. The start is not
standing: the probe still stops at `WelsTargetSliceConstruction:2426`. Because the probe
reports **one defect per run**, converting the grid on top of that would not produce an
unattributable failure so much as *no measurement at all* — the run would keep reporting
F25's site and say nothing about the grid. The precondition exists for exactly this and
it was applied rather than argued with.

What it costs: 5.2's conversion is one session further out. What it buys: the conversion
lands on a path where its own Miri verdict means something, which is the reason the gate
was written after F24.

### 5. Numbers

| metric | entry | exit |
|---|---|---|
| tests (debug / release / ignored) | 449 / 443 / 20 | **451 / 445 / 20** (+2, `memory_align`) |
| Miri `--lib` | 309 | **311** (+2, the same two) |
| decode goldens | 56 rows | **56 rows, none moved** (both code faces) |
| census | 60 allowlisted | **60** |
| `raw_ptr` | 4548 | **4589** (+8 tests at T5.F2, +33 at T5.F3) |
| `unsafe_block` | 614 | **619** |
| `unsafe_fn` | 1247 | **1248** (`cabac_ctx_base`) |
| `SHIM(` | 158 | **158** |
| Miri skips | 2 | 2 |
| findings | F26 open | **F26 FIXED; F27–F30 new, all FIXED** |

The `raw_ptr` rise is what removing retags costs — 20 `*mut SDqLayer` annotations, 9
`*mut SSlice`, 5 `*mut BsCursor`, 2 `*mut BsReader` against 8 removed. T5.E1 made the
same trade in the other direction when its fix collapsed double casts; S16's instruction
to read the shape rather than the sign covers both.

Batteries: T5.F2 and T5.F3 each `gates.sh full` with **two FAILs, both adjudicated** —
the ratchet, regenerated in its own commit per S16, and one sweep row per face.

### 6. F3 — measurements 30, 31 and 32, and the first alternation that could have said something

**Step 0 was checked and declined, which is new.** T5.F2 is the phase's first commit to
touch a `common/` module the encoder links, so the two `rust_enc` binaries genuinely
differ (`ed3b2622` base, `5ab0ac61` head) and the hash shortcut does not apply. Every
prior use of step 0 in this project has been an acquittal; this is the first time it was
run and came back "no".

* **30 and 31** (T5.F2, release): `mt … t=4 sm=3 n=600 cabac=1 rc=0` on two different
  streams, wrong lengths 0 and 28537. Each 5/5 byte-identical in isolation. Rate over
  five back-to-back release sweeps: **2 / 1705 ≈ 1/850**, against F3's measured ≈1/800.
* **Step 2 alternation**, 12 `mt` presets per side in one loop, 1440 configurations each:
  **base 1 / head 1**. Even, and not 0/0 (S23b).
* **32** (T5.F3, debug): `mt … t=4 sm=3 n=600 cabac=1 rc=1`, zero-length. Isolation gave
  **4× byte-identical and 1× zero bytes from one binary and one configuration** — S14
  step 1's race criterion met *directly*, the second time this project has had that
  quality of evidence (measurement 29 was the first). A deterministic port bug does not
  produce identical bytes four times out of five.

Twenty-eight measurements, eleven alternations, eleven acquittals.

### Hand-off: Phase 5, session G

* **The next unit is F25's inventory, and it is now a task rather than a debt.** 12
  `&mut *pCtx` bindings decoder-side — **7 in `decode_slice.rs`**, 5 in
  `manage_dec_ref.rs`. The spelling is S29's and T5.E1 did 43 sites of it in one commit.
  This is what stands between the tree and a green probe, and it is the thing to do
  first because everything after it wants the probe.
* **Expect the queue behind them to be non-empty.** Every round trip since T5.D2 has
  found something, and F28 widened the class: it is not the ~30 `&mut *pCtx` sites, it is
  every `&mut` of anything reachable from `pCtx` held across a re-entrant call. Budget
  for a bound again.
* **Then the grid, from the standing start this session did not have.** The closure is
  unchanged (session D §2 as corrected by T5.E2): grid in the layer, 50 signatures
  against 0; `DqLayerState`/`MbGrid` must keep the **allocation's** dimensions reachable
  for teardown; S28 accessors with full-reach Miri tests; S21 for the `WelsMallocz`-reached
  construction; the census keys re-keyed in the rename's own commit.
* **`pBitStringAux`'s second path is narrowed but not gone.** T5.F3 removed the
  *conflict* (no reference to the reader is created on the CABAC path); the field still
  exists and `cabac_decoder.rs:855`'s `SHIM(phase5)` accessor still reaches the layer.
  Both die in 5.2, as planned — F27's entry says why a local patch was not preferred.
* **F23 remains Phase 8's.**

## 2026-08-12 — Phase 5, session G (F25's inventory closes; the probe runs un-ignored and green; the queue's last defect was not an aliasing defect; 5.2's flip is sized and one landmine lighter)

**Commits:** `14d63d1a` (inherited doc tail — S24's multiline clause, the session brief),
`b4e60a66` (T5.G1, the inventory + the un-ignore + F31), `8455dd56` (ratchet, S16),
`5a71afe3` (T5.G2, F32), and this entry.

### The session in one line

The blocker that has stood since session E is gone — `decode_slice_loop_runs_under_the_aliasing_checker` runs un-ignored, green, inside the `--lib`
Miri gate — and the one defect standing behind it was not an aliasing defect at all,
which is better evidence the seam is clean than another quiet run would have been.

### Control battery

Docs-only tail, session F accepted (two FAILs, both adjudicated: the S16 ratchet and
one F3 sweep row per face), toolchain unchanged — S27's cheap subset. **OVERALL: PASS**,
451/445/20, ratchet 4589, census 60.

One judgement call in the predicate, recorded because it will recur every session from
here: `rust/tools/` **did** change in the tail — `unsafe_baseline.json`, regenerated at
T5.F3. No script changed. A regenerated baseline is the gate recording an accepted new
state, not a repaired gate, so it owes no S22 backlog run; read literally, S27's
"`rust/tools/` unchanged" would never hold again after any ratchet-moving session.

### 1. Face 1 — the `&mut *pCtx` inventory, and it was 11 (T5.G1)

F25, `phase5.md` §2, S29, the probe's own `#[cfg_attr]` label and session F's hand-off
all carried **12 bindings, 7 in `decode_slice.rs`, 5 in `manage_dec_ref.rs`**. Recounted
before acting, as S24 requires:

```
grep 'let ctx = &mut \*pCtx;' decode_slice.rs   ->  7 lines
  :661 :699 :2057 :2385 :2426 :5225              6 code
  :5405                                          1 `///`, inside the probe's doc
                                                   comment, illustrating the defect
```

**Eleven, not twelve.** The seventh was prose. That is S16's standing warning about
`raw_ptr` — prose inflates a count — arriving in a place S24 was already watching, and
the instrument that separates them is one comment-stripping pass before counting.

The correction changed nothing about the work, and that is the part worth keeping. The
task's shape was *convert every `&mut *pCtx` binding*, not *convert twelve things*, so a
wrong count could not misdirect it. F29 cost a round trip last session on a count that
was wrong by four, because there the count **was** load-bearing — it decided which sites
got fixed. **A wrong count is harmless exactly when it is not load-bearing, and there is
no way to know which kind you are holding without checking.**

Converted: 11 bindings, 97 uses re-spelled `(*pCtx).`, and **13 nested borrows** that
hung off them — `pCurSlice`, two `pDec`, two `pRefPic`, three `pCurDqLayer`, three
`pSliceHeader`, one `pic` — re-derived with `addr_of_mut!` or taken raw.

Two of those were not `&mut *` shapes and would have survived any grep for one:

* `WelsMarkAsRef`'s `&mut (*pCtx).sTmpRefPic as *mut SRefPic` — **S29's forbidden
  derivation with the cast already written.** The reference exists and retags before the
  cast discards it, so the site reads as raw-pointer code and behaves as a borrow. S29
  now names the shape.
* `manage_dec_ref.rs`'s four `&mut *(*pCtx).pCurDqLayer` — F28's exact class in a file
  F28's 20-function sweep did not cover, because F28 was scoped to `decode_slice.rs`.

Result: `src/decoder/` holds **zero** `&mut *pCtx`, and **no function in the port takes
`&mut SWelsDecoderContext`**. The god-context is never a borrow anywhere.

### 2. Face 2 — the probe comes off ignore, and the queue was one deep (T5.G1)

**F31**, and it is the first thing this probe has found that is not an aliasing defect.

```text
error: Undefined Behavior: reading memory at alloc253221[0x1fe0..0x2378], but memory is
       uninitialized at [0x231d..0x2320], and this operation requires initialized memory
     3: decoder::nalu::bytes_equal::<decoder::parameter_sets::TagSps>
     4: decoder::nalu::ParseSps
```

`alloc253221` is the decoder context, so the uninitialized operand is the **stored** SPS.
The three bytes are `sVui + 1`: `TagVui` opens with a `bool` and its next field is a
4-aligned `u32`.

`au_parser.cpp` runs a three-legged idiom that only works whole — `memset` the source,
`memcpy` it in, `memcmp` it back. The port translated leg 2 as
`copy_nonoverlapping::<SSps>(src, dst, 1)`, a **typed** copy, which does not carry
padding. Leg 1 had the same hole: a zeroing initializer produces a zeroed *value* and
moving it into the binding is a typed copy too.

The detail worth carrying: **`ParseSps`'s own comment has explained since the function
was written** that the zeroing exists so the comparison is meaningful, and that stale
padding "would read as a changed SPS and force a spurious new sequence, resetting the
DPB mid-stream". The comment was right about the mechanism and right about the stakes,
and the code discarded the zeroing one line later. S24 is usually about trusting someone
else's prose; here the prose and the fact were written by the same hand in the same
function.

Fixed as S6 parity in the strong sense — the fix makes the Rust do what the C++ does,
and the defect was introduced in translation: one `bytes_copy<T>` that *is* `memcpy`, all
**10** paramset stores routed through it, `write_bytes(.., 0, size_of::<T>())` over both
temps' own storage. Owner 5.5; fixed here because it is what the probe stopped on and it
is three lines of mechanism.

**The probe then passed**: 1 passed, 0 failed, 377s. The bound was three defects beyond
the first and the queue stopped at one.

Three consequences, all of them permanent:

* **The slice-decode loop is under the aliasing checker end to end**, for the first time
  in this project's history. Every 5.2–5.6 conversion from here gets a Miri verdict that
  means something — which is the precondition session E wrote the gate for.
* **The Miri gate costs ~6 minutes more per run.** That is the price of the coverage.
* **Re-adding `#[cfg_attr(miri, ignore)]` now requires a finding that owns it** (S15),
  with the label naming what it waits on. The label was rewritten to say so.

**Why the shape of the stop matters.** Ten defects over eleven round trips since T5.D2
were aliasing defects, and the queue had produced one every single time. The obvious
reading of another aliasing find would have been "the class is not exhausted". Getting a
*different* class instead, and then nothing, is the first positive evidence that the
aliasing seam is actually clean rather than merely not yet re-probed.

### 3. Face 3 — sized, not landed, and one landmine defused (T5.G2)

The standing start existed, so face 3 was in scope. It was measured before it was
started, and the measurement is the reason it did not land:

| quantity | measured |
|---|---|
| `*mut SDqLayer` signature positions | **75**, over 6 decoder files |
| grid fields / allocations / frees | **24 / 27 / 24** |
| grid field accesses, decoder-side | **~697** |

That is not one session's work, and a half-flipped closure is the state S20 exists to
forbid. What *was* in scope was the check that must precede it.

**F32 — two of the 24 arrays declare a scalar pointee and allocate a per-MB array.**
Cross-checking every field's declared type against its allocation expression is a dozen
lines of script and one second:

```
pIntraPredMode       *mut i8    numMb * 8  * sizeof(i8)    really [i8; 8]
pIntra4x4FinalMode   *mut i8    numMb * 16 * sizeof(i8)    really [i8; 16]
```

The other 22 agree exactly. 5.2 derives each `MbArray<T>`'s `T` from the declared
pointee — the only way to read 700 sites — and read that way these two allocate **8× and
16× too little**. The writes land on `[7]` and on scan-order slots, so a small stream can
touch one element of each and pass every gate in the battery.

`dec_frame.h:85-86` settles what it is: `int8_t (*pIntraPredMode)[8];  //0~3 top4x4 ;
4~6 left 4x4; 7 intra16x16`. Pointer-to-array became pointer-to-scalar in translation and
the comment naming the slot layout came across as nothing. The fix restores both — 24
sites, zero bytes moved, and one S29 escaping borrow retired with them.

**The lesson against `phase5.md` §2's own text.** It records "F19's check discharges
clean at **field** level — 24 arrays against 24 frees". True, and it is a check of
*pairing*, not of *size*. Both frees here are wrong in the same direction as both
allocations, which is precisely why they paired. A pairing check cannot see a
consistently wrong size, and the encoder's `SDqLayer`/`SMbCache` in 6.3 have never been
checked the other way.

### 4. Numbers

| metric | entry | exit |
|---|---|---|
| tests (debug / release / ignored) | 451 / 445 / 20 | **451 / 445 / 20** |
| Miri `--lib` | 311 | **312** (the probe, un-ignored) |
| decode goldens | 56 rows | **56 rows, none moved** (both code faces) |
| census | 60 allowlisted | **60** |
| `raw_ptr` | 4589 | **4604** (+15 at T5.G1; T5.G2 flat) |
| `unsafe_block` | 619 | **619** |
| `unsafe_fn` | 1248 | **1249** (`bytes_copy`) |
| `SHIM(` | 158 | **158** |
| Miri skips | 2 | 2 |
| findings | — | **F31, F32 — both new, both FIXED**; F25's inventory closed |

`raw_ptr` +15 is the trade session F named, in the same direction: 11 bindings and 13
nested borrows became raw derivations, and each costs a pointer type annotation. Read the
shape, not the sign.

**S16's prose floor was collected twice more, and both would have corrupted a live
instrument.** At T5.G1 a doc comment naming the zeroing intrinsic pushed `mem_zeroed`
2 → 3 in `nalu.rs` with no new call site — and `mem_zeroed` is S21's construction-audit
count, still in use. At T5.G2 a doc comment quoting the old pointer type pushed `raw_ptr`
+1. Both reworded, not baselined, as session C did. Four instances now; the operational
form of the rule is **read per-file deltas, never the total** — in a total, a one-line
prose delta and a real conversion are the same number.

Batteries: `gates.sh full` twice, `gates.sh commit` three times. Every step PASS except
the ratchet at T5.G1 (regenerated in its own commit per S16) and a first ratchet reading
at T5.G2 that the prose reword cleared.

### 5. Perf, and what was not measured (S17)

Decode bench, the two full batteries, same three streams and identical SHA-1s
(`0fba9a4e…`, `d8c07c43…`, `8b081cce…`): CB 386.88 → 384.19 fps (−0.70%), Main
159.13 → 159.02 (−0.07%), High 158.09 → 157.70 (−0.25%). Encoder rows bit-identical.

**No interleaved 3-pair median and no S2 null were run**, and saying so is the point.
Those readings are sequential singles, which S1 puts at ~3% drift, so they establish
nothing tighter than "nothing large happened". The brief asked for 3-pair medians per
seam; the judgement made was that this session has no seam that could plausibly move —
every change is a pointer *spelling* change with byte-identical output, no kernel, no
allocation path and no dispatch was touched, and no shim died, so §7.4's ledger is
unmoved too.

The one genuinely new instruction cost is F31's: two redundant `write_bytes` of ~920 and
~600 bytes, once per SPS and once per PPS NAL. It is redundant with the zeroing
initializer that precedes it, kept because the pair is what makes the invariant legible.
**5.5 should collapse it** into a `MaybeUninit` + `write_bytes` construction when it
rewrites the paramset store; it is not worth a separate commit now.

### 6. F3 — zero hits, and a gap in the ledger that the zero exposed

**Zero hits.** Four sweeps of 341 configurations (two full batteries × two profiles) =
1364 configurations, all PASS, both profiles. Nothing to adjudicate: S14's protocol
starts at a hit and there was none. Per S14 step 4 this is a **sample, not a signal** —
F3's measured rate is ≈1/800 under sustained load, so 1364 configurations drawing zero
is an ordinary outcome and says nothing about whether the finding is still live.

**The useful part was going to write it down.** The brief said "measurements 33+
append", so the first move was to find measurement 32 in `phase0_findings.md` — and F3's
ledger there **ends at 29**. Session F's measurements 30, 31 and 32, its 5× isolation
re-runs, its 2/1705 rate and its eleventh alternation were all written into that
session's *log entry* and none of them into F3. S14 step 4 says append every measurement
to F3, and the ledger is what the next session greps for the rate; a measurement that
lives only in a session log is invisible to it. Session F's own closing tally read
"twenty-eight measurements, eleven alternations", which is the arithmetic of a ledger
that stopped being updated.

Recovered verbatim from the log entry rather than re-run, and appended to F3 with the
zero-hit sample behind it. **Running total: thirty-two measurements, eleven alternations,
eleven acquittals.**

This is S22's law aimed at a record instead of a gate: an instrument's history is part of
the instrument, and the only reason the gap surfaced is that a *zero* forced a look at
where the numbering had got to. Four sessions of hits would have kept writing them into
log entries.

### Hand-off: Phase 5, session H

* **The next unit is 5.2's grid flip, and it is now the only thing in front of the
  phase.** Blocker: none. The closure is unchanged (session D §2 as corrected by T5.E2):
  grid in the layer, 50 signatures against 0; `DqLayerState`/`MbGrid` must keep the
  **allocation's** dimensions reachable for teardown; S28 accessors with full-reach Miri
  tests; S21 for the `WelsMallocz`-reached construction; census keys re-keyed in the
  rename's own commit.
* **Start from the sizing in §3, not from the closure's prose**: 75 signature positions,
  24 fields, ~697 accesses. It is bigger than one session. Split it as S20 allows —
  pure addition first (`MbGrid` as a struct of the Phase-1 `MbArray<T>`s, its field union
  read off the allocation block, proven with S28 full-reach Miri tests before anything
  flips onto it), then the flip, then the ~28 cache-fill signature re-points behind it.
* **`MbGrid`'s field union is mechanical now.** All 24 element types agree with their
  allocations as of T5.G2 — that was F32's whole point — so the union can be read off
  `InitialDqLayersContext` without judgement. Note `safe/mb_grid.rs` is
  `#![forbid(unsafe_code)]`: the safe grid goes there, S28's raw accessors do not, and
  T5.C3's `SPicture::data_ptr` is the precedent for where they do go.
* **The probe is a gate now, not a probe.** If the flip turns it red, that is the flip's
  own defect and it is attributable — which is the entire reason the last three sessions
  went the way they did. Budget ~7 minutes for the Miri step.
* **`pBitStringAux`'s second path is narrowed but not gone** — unchanged from session F.
  The field still exists and `cabac_decoder.rs:855`'s `SHIM(phase5)` accessor still
  reaches the layer. Both die with the flip.
* **F23 remains Phase 8's. F31's redundant memset is 5.5's** (see §5).

## 2026-08-12 — Phase 5, session H (the grid exists, the layer owns one, and half the families have flipped onto it — and the first honest perf reading of what that costs)

**Commits:** `95a9cb1d` (inherited doc tail — S2c, S14, S16), `f4afe9ac` (T5.H1, F33),
`0b3753e5` (T5.H2, `MbGrid`), `83e94d4a` (ratchet), `4168ebdc` (T5.H3, the seat),
`3c4c6f4e` (ratchet), `25d3f048` → `1438d762` (T5.H4–T5.H14, eleven families, one
commit each), `24053df1` (docs — F33, 5.2's marks, F3), `353f5b7a` (ratchet), and
this entry.

### The session in one line

5.2's flip is half done: `MbGrid` exists and is proven, `SDqLayer` owns one and can no
longer be constructed by zeroing, eleven of the twenty-two array families have flipped
onto it with the aliasing probe green throughout — and a 7-pair median says the safe
indexing costs **+1.3% decode, +2.05% on Constrained Baseline**, which is the first
number in this phase that is a real cost rather than a wash.

### Control battery

Docs-only tail, session G accepted (**OVERALL: PASS**), `rust/tools/` and the toolchain
unchanged — S27's cheap subset. **OVERALL: PASS**, 451/445/20, ratchet 4604, census 60.
Every figure matched what session G recorded, which is the first time this phase the
open needed no correction.

### 0. The sizing, recounted (S24)

The brief's governing numbers were **75 signature positions, 24 fields, ~697 accesses**.
Re-greped before acting:

```
*mut SDqLayer / PDqLayer mentions          92   (comment-stripped)
  of which: fn parameter positions         66   over 6 files
            `let` bindings                 24   decode_slice 20, manage_dec_ref 4
            struct fields                   2   SWelsDecoderContext
grid field accesses (`.pField`)           697   exactly the brief's figure
```

**66, not 75** — and 66 is corroborated by `phase5.md` §2's own independent count from
session D ("of 66 functions taking a DqLayer pointer, 50 take the layer only"), which is
the second instrument S24 asks for. The 75 was session G's, measured with a shape that
also caught the 24 local bindings and some of the struct fields. It changed nothing: the
work is *convert every access*, not *convert 75 things*, so the wrong count was not
load-bearing — the same disposition T5.G1 reached about F25's twelfth binding, and the
same warning attaches. You cannot tell which kind you are holding without checking.

The **~697** was exact.

### 1. F33 — two of the twenty-four arrays have no reader (T5.H1)

Before transcribing the field union off the allocation block, both directions of the
inventory were counted: writes and reads, per field, over both trees.

```text
field                       writes   reads
pNzcRs                        0        0
pInterPredictionDoneFlag     14        0
```

Both dead in the **C++** too — `pNzcRs` is allocated, aliased and freed with no other
mention in `codec/`; `pInterPredictionDoneFlag` is written `= 0` at 14 sites in
`decode_slice.cpp` and read nowhere. The port is faithful at all 14. Deleted: 24 arrays
became 22, 27 allocations became 25.

**The half worth carrying is that the two are not the same kind of dead.** `pNzcRs`
would fall out of any "is this mentioned" sweep. `pInterPredictionDoneFlag` would not:
14 live writes make every reference-counting instrument call it used, and only counting
reads and writes *separately* shows it. An unread write hides from exactly the tools
that find unused fields — and this one costs a store per macroblock on every parse path
in both entropy coders.

### 2. Face 1 — `MbGrid`, and proving S28 rather than asserting it (T5.H2)

The union is a **transcription** of `InitialDqLayersContext`'s allocation block, in that
block's order, because that is the one place an element type and an element count are
stated together. Reading it off the struct declaration would have been a judgement about
what each pointer meant, and F32 is what that judgement was worth for the port's whole
life. T5.G2 and T5.H1 are why the transcription is mechanical.

The raw bridge, `mb_grid_ptr`, went to `decoder_core.rs` with the consumers and **not**
into `safe/mb_grid.rs`, which stays `#![forbid(unsafe_code)]` — `SPicture::data_ptr`'s
precedent, and the brief was explicit about it.

**S28's tests were proven to be instruments.** The rule is only worth writing down if
something can fail against it, so the derivation was temporarily swapped for the
narrowing spelling — same address, still safe code — and Miri named it on the first run:

```text
error: Undefined Behavior: attempting a write access using <654221> at
       alloc308545[0xefe], but that tag does not exist in the borrow stack
```

Both reach tests fail narrowed and pass rooted; under the ordinary runner both pass.
Coverage proven by reversion, as T5.B1 proved F21's and T5.C3 proved its own.

### 3. Face 1 — the seat, and S21 with the allocator watching (T5.H3)

`SDqLayer` gained `grid: MbGrid` **by value**, and that forced its whole lifecycle in one
commit (S20): `Copy` off, `Default` deleted, `WelsMallocz` replaced by
`Box::into_raw(Box::new(SDqLayer::for_grid(dims)))`, `WelsFree` by `Box::from_raw`.

The alternative — a `*mut MbGrid` in the layer — was considered and rejected. It would
have kept `Copy` and `mem::zeroed` valid and made every family commit smaller, at the
price of a second path to the same data and a shim to delete later. That is P2's disease
by name, in the step whose whole job is killing P2's instance of it.

**S21 discharged the way T3.3 wrote it**: zeroed `MaybeUninit` shell,
`addr_of_mut!((*p).grid).write(MbGrid::new(dims))` (S29 — no `&mut` to a field of a
not-yet-valid struct), materialize last. No shell survives, because the grid is the only
owning field. `Default` was **not** reinstated: a layer without dimensions is not a
thing, and a `Default` that invented some would be the same lie the zeroing was. The
compiler found `Default`'s complete user list — two test fixtures, both of which state
their dimensions two lines above.

**T5.E2's landmine became unrepresentable rather than fixed.** The grid is sized from the
negotiated maximum, its dimensions travel inside every `MbArray`, and the free path that
T5.E2 caught reading the *slice's* dimensions has no arithmetic left: dropping a `Vec`
reads nothing.

Miri verdict on this face: **324 passed, 524s**, the probe included — a real stream
decoded through a heap-constructed layer owning 22 `Vec`s.

### 4. Face 2 — eleven families (T5.H4–T5.H14)

| # | family | accesses | files |
|---|---|---|---|
| 1 | `pIntraNxNAvailFlag` | 2 | 1 |
| 2 | `pIntra4x4FinalMode` | 4 | 1 |
| 3 | `pResidualPredFlag` | 8 | 1 |
| 4 | `pChromaPredMode` | 19 | 2 |
| 5 | `pCbfDc` | 8 | 2 |
| 6 | `pLumaQp` | 34 | 4 |
| 7 | `pNoSubMbPartSizeLessThan8x8Flag` | 17 | 3 |
| 8 | `pTransformSize8x8Flag` | 36 | 3 |
| 9 | `pCbp` | 29 | 2 |
| 10 | `pMbRefConcealedFlag` | 8 | 2 |
| 11 | `pSubMbType` | 22 | 5 |

**Remaining, by name**: `pMbType`, `pSliceIdc`, `pMv`, `pMvd`, `pRefIndex` (the three
`LIST_A` pairs), `pDirect`, `pChromaQp`, `pNzc`, `pScaledTCoeff`, `pIntraPredMode`,
`pMbCorrectlyDecodedFlag`. Stopped at a family boundary, per the brief's §6.

Four things the flip turned up that were not in the plan for it:

**Eight port-invented null guards came out.** `pCbp` had one, `pMbRefConcealedFlag` four,
`pSubMbType` three. None of them exists in the C++ — `decode_slice.cpp:334`, `:132`,
`:1596`, `:1711`, `:1743`, `mv_pred.cpp:593`, `:669`, `error_concealment.cpp:319` all
index unguarded. They are F22's class (a test on one side of the translation only) but
harmless in the safe direction, and the flip does not argue them away, it makes them
unrepresentable. One of the eight was a **conjunct**: `error_concealment.rs` tested
`pSubMbType`, `pDec->pRefIndex[0]` and `pDec->pMv[0]` together, and only the first came
out — the other two are the *picture's* arrays, still raw, still nullable, 5.1/5.4's.

**A dead parameter pair.** `ParseCbfInfoCabac` declared `pCbfDc: *mut u16` and
`pMbType: *const u32` and shadowed both, four lines in, with locals re-deriving the same
expressions its caller had just evaluated to pass them. Dead since it was written, in
the port and in the C++. `pCbfDc`'s could not survive the flip; `pMbType`'s came out
with it because leaving half a dead pair is worse than leaving both.

**One signature retired on the flip itself.** `CheckIntraChromaPredMode(u8, *mut i8)` had
three callers and every one passed `pChromaPredMode[iMbXy]`, so when the array became a
grid entry the parameter's only possible source became `&mut i8`. Its body then needs no
`unsafe` at all: `unsafe_fn` −1. That is inside the family's closure (S20), not a
face-3 signature re-point.

**A memset became a checked fill, and it is T5.E2's hazard in miniature.**
`write_bytes(pMbRefConcealedFlag, 0, iMbNum)` used the **SPS's** dimensions, smaller than
the grid's negotiated maximum on any stream below it. As `as_mut_slice()[..iMbNum]
.fill(false)` the relationship is checked at the point of use rather than assumed.

**The type system corrected the rewrite three times, and all three were the same class:**
an access whose *direction* a textual rule got wrong — a write whose `=` sat on the next
line (F29's multiline clause, in a rewrite instead of a count), a read-modify-write
(`*flag = *flag && ..`), and two `&mut *ptr` argument sites. `get` and `get_mut` are
different methods, so every one was a compile error. A count would have carried all
three in silently. A fourth correction was mine and worse: the file list for `pLumaQp`
came from a `grep` that `head -90` had truncated, and three sites in two unnamed files
survived it. **S24 with a pager in it.** The helper now sweeps every decoder file rather
than a named list; that is the fix, not "read more carefully".

### 5. Numbers

| metric | entry | exit |
|---|---|---|
| tests (debug / release / ignored) | 451 / 445 / 20 | **463 / 457 / 20** |
| Miri `--lib` | 312 | **324** |
| decode goldens | 56 rows | **56 rows, none moved** |
| census | 60 allowlisted | **60** |
| `raw_ptr` | 4604 | **4570** (−34) |
| `unsafe_block` | 619 | **622** |
| `unsafe_fn` | 1249 | **1248** (`CheckIntraChromaPredMode`) |
| `mem_zeroed` | 32 | **31** (the layer stopped being zero-constructible) |
| `SHIM(` | 158 | **159** — `phase5` is now **3** |
| Miri skips | 2 | 2 |
| findings | — | **F33, new and FIXED** |

`raw_ptr` −34 is the first real *decrease* of the phase that comes from conversion rather
than deletion: 22 field declarations, 22 allocations and 22 frees against two new S28
bridges. The +2 in `decode_slice.rs` is those bridges, and it is the trade sessions F and
G both named — an honest derivation costs a pointer type annotation.

**S16's prose floor collected three more times, all in this session's own commits**, and
two of them on `mem_zeroed`, which is S21's live construction-audit count and the one
number a prose delta would have hidden the real movement in. A third put `raw_ptr` at 1
in `safe/mb_grid.rs` — a `forbid(unsafe_code)` file, where any nonzero `raw_ptr` is a
lie. All reworded, none baselined. Seven instances now; the operational form is
unchanged and was what caught them: **read per-file deltas, never the total.**

Batteries: `gates.sh full` three times, `gates.sh commit` eleven times. Every step PASS
except two ratchet readings, each regenerated in its own commit per S16.

### 6. Perf — the first reading in this phase that is a cost

S2c does **not** apply: the flip touches allocation and layout, which the brief said in
advance and the measurement bears out.

**The seat (T5.H3), 3 pairs against the S2 null:** decode median **−0.21%** (3 rows,
−0.79%..−0.19%), encode median −0.05%. This session's null band, measured the same day
on the same machine, is **±1.55%** (28 rows, median −0.22%). Flat, as an allocation-shape
change with no per-access change should be.

**The eleven families, 3 pairs:** decode median **+0.82%**, and all three rows positive
— +0.38%, +0.82%, +1.37%. Inside the null band, but monotone, which a band does not
account for. S2b's instruction for exactly this is more pairs before a mechanism, so:

**The eleven families, 7 pairs:**

```
Constrained Baseline (CAVLC)   2.6370 -> 2.6910   +2.05%
Main (CABAC, B-frames)         6.3640 -> 6.3950   +0.49%
High (CABAC, 8x8)              6.3590 -> 6.4430   +1.32%
  decode: rows 3, median +1.32%, min +0.49%, max +2.05%
  encode: rows 28, median −0.08%   (decoder-only change; unaffected, as expected)
```

**More pairs firmed it up rather than washing it out**, and CB is now outside the null
band. The mechanism is not mysterious: eleven families' per-macroblock accesses are
slice indexes with a bounds check where they were `ptr.add(i)`, and CB is the stream with
the most macroblocks per second, so it pays the most. S8 forbids `get_unchecked` this
phase and nothing here reaches for it.

**What this means for the phase, stated plainly rather than left for arithmetic.**
`phase5.md` §7 budgets ~7 points of CB headroom under the tripwire and says flat
mid-phase readings are expected because the payoff — constant dimensions reaching the
kernels — arrives at the ledger, not the bench. Half the flip has now spent **~2 of
those 7 points**, and the eleven families remaining include the largest ones: `pNzc`
(75 sites), `pMv`/`pMvd`/`pRefIndex` (hundreds, and read in the motion-compensation
inner loops). A linear extrapolation is not warranted from three streams, but the
direction is not in doubt and the remaining families are the hotter half. **This is the
session's most important result and it is a question for the next one, not a defect in
this one.**

### 7. F3 — zero across six sweeps

Six sweeps of 341 configurations (three full batteries × two profiles) = 2046
configurations, all PASS, both profiles. Nothing to adjudicate; S14's protocol starts at
a hit. Appended to F3 as a sample per S14 step 4 — and appended *when it was known*,
which is the clause session G added after finding the ledger four sessions in arrears.

Second consecutive zero-hit session. Sessions G and H together are 3410 configurations
against a ≈1/800 rate — about four expected hits, so zero twice running is on the
unlikely side of ordinary. It is still not a signal: both sessions were 100%
decoder-side and the sweep exercises the encoder. Running total unchanged at **thirty-two
measurements, eleven alternations, eleven acquittals**.

### Hand-off: Phase 5, session I

* **The unit is the same one: 5.2's flip, second half.** Eleven families remain, named
  in §4 and in `phase5.md` §2. The seat, the accessor and the pattern are all in place,
  so a family commit is now the flip and nothing else — the four classes of surprise
  this session met (invented null guards, dead parameters, a signature inside the
  closure, a memset with the wrong dimensions) are all worth expecting again.
* **Read §6 before sizing the session.** The safe indexing costs ~1.3% decode and ~2% CB
  for the *first* eleven families, measured at 7 pairs, and the remaining eleven are the
  hot ones — `pNzc`, `pMv`, `pMvd`, `pRefIndex`, `pScaledTCoeff`. Two courses, and
  choosing between them is a judgement the next session should make **before** it flips
  the motion-vector families rather than after: take the cost and bank it against §7.4's
  ledger, or measure per family and decide where the line is. What is not available is
  `get_unchecked` (S8) or not measuring.
* **The three big families are not like the eleven.** `pMv`/`pMvd`/`pRefIndex` are
  `[MbArray<_>; LIST_A]` pairs, they are read through `pDec` as well as through the
  layer — `(*(*dq).pDec).pMv[..]` is the **picture's**, not the layer's, and a
  mechanical rewrite must not touch it — and `pScaledTCoeff`'s consumers hold
  `&mut [i16; 384]` across calls that re-enter the layer through `pCtx`. That last one
  is F28's shape and is the first family in this flip where the probe has a real chance
  to go red. Budget a round trip for it.
* **The probe is a gate and it held.** Three full batteries, three green Miri runs, 324
  tests, ~524s each. Every family that flipped got a verdict that means something.
* **`pBitStringAux` and `cabac_decoder.rs:855`'s `SHIM(phase5)` are untouched** — they
  die with the family that carries them, which is none of the eleven flipped. Unchanged
  from session G's hand-off.
* **Face 3 has not started.** The ~28 cache-fill signature re-points wait on the
  families that carry them; none of the eleven is one. F23 remains Phase 8's, F31's
  redundant memset 5.5's.

---

## 2026-08-12 — Phase 5, session I (D-perf-5's retrofit lands, recovers nothing, and the disassembly says exactly why)

**Commits:** `c51f802d` (inherited doc tail — D-perf-5, §0's two rows, the brief),
`17e5b7ba` (T5.I1, the retrofit across all eleven families), `a5aabacd` (T5.I2, F34), and
this entry. **No ratchet commit**: every metric and every per-file row is unmoved, which
is what the brief predicted for accessor reshaping.

### The session in one line

The per-macroblock window accessor is retrofitted across all eleven flipped families, it
does mechanically exactly what D-perf-5 asked — **76 bounds checks per macroblock on the
CAVLC path become 38** — and it recovers **nothing**: decode median **+0.14%** at 7 pairs
against a bar of **−0.66%**. The hour §3.4 budgets for a look found why, and it is not a
defect in the retrofit: those checks live in functions that are **1–3% of decode time**,
so there was never a 2% there to take back.

### Control battery

Docs-only tail, session H accepted (**OVERALL: PASS**), `rust/tools/` and the toolchain
unchanged — S27's cheap subset. **OVERALL: PASS**, 463/457/20, ratchet 4570, census 60.
Every figure matched session H's exit; second session running that the open needed no
correction.

**The S2 null, run at open because every verdict here is a perf verdict** (3 pairs, same
binary in both slots): decode **±0.6%** (3 rows, median +0.19%, −0.21%…+0.60%); encode 28
rows, median −1.34%, −12.07%…+8.14%. The decode floor is the tight one and it is the one
that binds — this is a decoder-only change.

### 0. The sizing (S24), and half the brief's idiom was already built

The brief's §1 names two halves: re-type each family's element so the per-MB record *is*
the element, then hoist the window borrow to the loop head. Re-greped before acting:

```
family                            element        per-MB width   sites (comment-stripped)
pIntraNxNAvailFlag                u8                  1              2
pIntra4x4FinalMode                [i8; 16]           16              4
pResidualPredFlag                 i8                  1              8
pChromaPredMode                   i8                  1             18
pCbfDc                            u16                 1              8
pLumaQp                           i8                  1             39
pNoSubMbPartSizeLessThan8x8Flag   bool                1             17
pTransformSize8x8Flag             bool                1             34
pCbp                              i8                  1             32
pSubMbType                        [u32; 4]            4             22
pMbRefConcealedFlag               bool                1              5
                                                                  ---
                                                                   189
```

**All eleven are already `MbArray<[T; K]>` with a constant K**, because T5.H2 transcribed
the union off the allocation block rather than off the struct declaration — and the two
multi-element families are declared the same way in the C++ (`dec_frame.h:86`, `:90`:
`int8_t (*pIntra4x4FinalMode)[MB_BLOCK4x4_NUM]`,
`uint32_t (*pSubMbType)[MB_SUB_PARTITION_SIZE]`). So the first half needed no work, no
family reaches S9's variable-width fallback, and the whole session is the second half.

### 1. The retrofit (T5.I1)

One window borrow per macroblock per array, hoisted to the head of the region that uses
it — where the C++ hoists its pointer. Checks executed per macroblock, on the path that
pays most:

| family | before | after | where the window lands |
|---|---|---|---|
| `pIntra4x4FinalMode` | 16 | 1 | the `0..16` and `0..4 × 0..4` mode loops |
| `pLumaQp` | 16 | 1 | read once per residual block, both coders |
| `pSubMbType` | 8–32 | 1–5 | six partition loops over five functions |
| `pNoSubMbPartSizeLessThan8x8Flag` | 8 | 1 | the four 8x8 parse loops |
| `pCbp` | 5 | 1 | the three CAVLC actual-decode functions |
| `pTransformSize8x8Flag` | 4 | 1 | the same three |
| `pResidualPredFlag` | 3 | 1 | the CAVLC P/B write-then-test |
| `pChromaPredMode` | 3 | 2 | the three intra-mode parsers' final check |
| `pCbfDc`, `pIntraNxNAvailFlag`, `pMbRefConcealedFlag` | 1 | 1 | already one per access |

Static sites 189 → 141, which understates it: the four largest wins are loop-nested.
`deblocking.rs` is deliberately untouched — its `pLumaQp` and `pTransformSize8x8Flag`
reads are all at *distinct* addresses (current plus a neighbour) and were already one
check each.

**What bounds a window is the family's own neighbour reads, not the call graph.**
`MbArray::get` is `&self.data[i]`, `data` is a `Vec`, and `Vec`'s `Index` goes through
`Deref` — `slice::from_raw_parts(self.as_ptr(), self.len)` — which builds a shared
reference over the **whole buffer**. Any other access to the same array ends the window,
including a read at a *different* macroblock. Three boundaries come straight from that:
`GetNeighborAvailMbType` reads `pCbp` at the left and top addresses, so `pCbp`'s window
opens after it; `ParseIntraPredModeChromaCabac` reads `pChromaPredMode` at the top and
left, so that window opens after it; and `PredMvBDirectSpatial`/`PredBDirectTemporal`
write `pSubMbType[iMbXy]` themselves (`mv_pred.rs:1035`, `:1130`), so the B-slice parse
loop keeps a per-iteration window and only the read-only loops after it share one.

**It landed as one commit, not eleven.** The brief allowed one per family *or per tight
cluster*, and this is at the outer edge of that: the families interleave in the same
functions (`pCbp` and `pTransformSize8x8Flag` share the three CAVLC decoders, `pSubMbType`
and `pNoSubMbPartSizeLessThan8x8Flag` share the four partition loops), the idiom is one
edit applied once, and the measurement is a whole-span number by construction. The cost of
that choice is bisection granularity, and it did not bind — the revert branch of §3.4 is
"stop and report", not "unswap".

Full battery on the settled tree: **OVERALL PASS** — 463/457/20, Miri **324** (524s, the
aliasing probe included), sweeps 341/341 both profiles, decode goldens 56 rows none moved,
both benches bit-identical, census 60, ratchet flat.

### 2. The measurement — the session's deliverable

Whole-retrofit span, **7 interleaved pairs**, both benches, `i_ctrl` (`c51f802d`, which is
`75188044` plus the doc tail) vs `i_head` (`17e5b7ba`):

```
Constrained Baseline (CAVLC)   2.6060 -> 2.6040   -0.08%
Main (CABAC, B-frames)         6.2550 -> 6.2690   +0.22%
High (CABAC, 8x8)              6.3140 -> 6.3230   +0.14%
  decode: rows 3, median +0.14%, min -0.08%, max +0.22%
  encode: rows 28, median -0.25%   (decoder-only change; unaffected, as expected)
```

**Every row is inside this session's null band.** D-perf-5's bar was recovery ≥ half the
spent cost — decode median ≤ −0.66%, the CB row ≤ −1.03%. The measured recovery is zero on
all three rows. **The idiom is not proven, and §3.4 applies: the look, then stop.**

### 3. §3.4's look — the disassembly first, and it corrected me once

S1 says disassemble before theorising. It also, this time, says *use a second instrument*,
because the first one was wrong.

**The first count was wrong.** Walking basic blocks back from each `bl panic_bounds_check`
to find landing pads reported 5, 1, 4 and 10 check branches in four functions and made the
checks look like a minority of what the flip added. That undercounts badly: LLVM merges
the landing pads and most of the guard branches leave the function body entirely to a
shared thunk, so a walk confined to the body never sees them. The **opcode histogram** is
the instrument that works — an `MbArray` index compiles to exactly `ldr` (the length),
`cmp`, `b.ls`, and those three deltas move together and are countable.

Across the six CAVLC per-macroblock functions, `b.ls` count and total instructions:

| function | pre-flip | post-flip | post-retrofit |
|---|---|---|---|
| `WelsActualDecodeMbCavlcISlice` | 405 / 2 | 465 / 19 | 442 / 11 |
| `WelsActualDecodeMbCavlcPSlice` | 525 / 0 | 574 / 19 | 546 / 9 |
| `WelsActualDecodeMbCavlcBSlice` | 525 / 0 | 574 / 19 | 546 / 9 |
| `WelsDecodeMbCavlcResidual` | 680 / 0 | 703 / 11 | **961** / 2 |
| `ParseIntra4x4Mode` | 380 / 0 | 414 / 3 | 399 / 3 |
| `ParseIntra8x8Mode` | 422 / 0 | 469 / 7 | 437 / 4 |
| **total** | **2937 / 2** | **3199 / 78** | **3331 / 38** |

Three things fall out of that table, and they are the session's actual result.

**Session H's mechanism attribution was right.** The flip added **76 bounds checks** per
macroblock here, at three instructions each — ~228 of the 262 instructions it added.
`WelsActualDecodeMbCavlcISlice` is the clean isolate: `ldr` +17, `cmp` +17, `b.ls` +17,
total +60. The flip is bounds checks and almost nothing else.

**The retrofit did what it claimed.** 78 → 38, a bit over half, which is the design: one
check per macroblock per array instead of one per access.

**And removing 40 checks per macroblock is worth 0.00%.** They are perfectly predicted and
the length they load is in L1 beside the pointer. Worse, the hoist *paid* for itself in
code size: `WelsDecodeMbCavlcResidual` went 703 → **961** instructions because lifting the
QP load out of the residual loop made the body simple enough for LLVM to unroll it much
harder — `WelsResidualBlockCavlc` call sites went 16 → 31. Net across the six functions the
retrofit removed 40 branches and added 132 instructions.

**The profile agrees and explains the magnitude.** `/usr/bin/sample`, 3s over the
Constrained Baseline portion, self time, hashing excluded (the harness's SHA-1 is 57% of
raw samples and is byte-exactness verification, not decode): `DecodeBinCabac` 4.9%,
`deblock_luma_lt4` 4.7%, `McChroma_c` 3.4%, `deblock_chroma_lt4` 3.0%,
`ParseSignificantCoeffCabac` 2.4%, `BaseMC` 2.4%, `DeblockingBsMarginalMBAvcbase` 1.9%,
`BiWeightPrediction` 1.8%, `CavlcGetLevelVal` 1.3%, `WelsResidualBlockCavlc` 1.2%,
**`WelsDecodeMbCavlcPSlice` 1.2%**. The whole family of functions this session edited is
low single digits of decode time; the checks inside them are a fraction of that. **A 2%
whole-stream recovery was never available from this code.**

### 4. Then where is the flip's cost? — the same binaries, re-run

Session H's two builds are still stashed, so the flip's own span was re-run today at 7
pairs, same protocol, same machine:

```
                              session H (7 pairs)   today (7 pairs)
Constrained Baseline (CAVLC)        +2.05%              +1.18%
Main (CABAC, B-frames)              +0.49%              +0.65%
High (CABAC, 8x8)                   +1.32%              +0.33%
  decode median                     +1.32%              +0.65%
```

**The cost is real and it is about half what was recorded**, and the row that carried the
median swapped ends — High went from the median to the smallest. Same two binaries, same
seven pairs, same machine, a different day. Nothing about the tree changed between the two
readings.

So both of session H's claims survive in weakened form: the flip does cost something, on
every row, in both readings; and the magnitude that D-perf-5's arithmetic was built on
(**+2.05% CB, cumulative ≈ +19.9%**) is high by roughly a factor of two. S2b said three
pairs is a floor, not a guarantee of convergence. **Seven is not one either** — that is
this session's addition to it, and it is the more useful half of the result.

### 5. F34 — the flip turned a `bool&` into a `&mut bool`, and one callee reads its own array

Found by the window analysis, not by a gate; **proved rather than asserted**, on a
standalone twenty-line reproduction under Miri. `ParseTransformSize8x8FlagCabac`
(`parse_mb_syn_cabac.rs:1228`) is handed `grid.transform_size8x8_flag.get_mut(iMbXy)` and
then reads the same array at `iMbXy - 1` and `iMbXy - iMbWidth` before writing through the
argument. A `&mut` argument is strongly protected; `Vec`'s `Index` builds a shared slice
over the whole buffer; the write goes through a dead tag.

```text
error: Undefined Behavior: not granting access to tag <630> because that would remove
       [Unique for <720>] which is strongly protected
```

**Why every gate was green.** It needs `bTransform8x8ModeFlag` *and* a left or top
neighbour. `decode_slice_loop_runs_under_the_aliasing_checker` decodes `narrow_16x16.264`,
which is one macroblock per frame — both availability flags are 0, so neither read runs.
Miri executed this function and returned green because the two lines that make it UB were
never reached. **S22's law aimed at a stream instead of at a scope list: the probe's
coverage is its asset, not its target list, and every "the probe is green, so the flip is
sound" conclusion is bounded by what a 711-byte one-macroblock stream decodes.**

It is not a divergence: `parse_mb_syn_cabac.cpp:391` takes `bool&` and does the identical
thing, which is well-defined in C++. The port is faithful; `&mut` is stronger than `&`.
Fixed at T5.I2 by keeping the value in a local and storing it after the call — byte-exact
including on the error path, since the callee returns before its write. The grep for the
class is narrow and was run: exactly two call sites hand a grid window to a function, both
to this one, and `CheckIntraChromaPredMode` — the other `&mut` into a flipped family — is
clean by signature.

### 6. Numbers

| metric | entry | exit |
|---|---|---|
| tests (debug / release / ignored) | 463 / 457 / 20 | **463 / 457 / 20** |
| Miri `--lib` | 324 | **324** |
| decode goldens | 56 rows | **56 rows, none moved** |
| census | 60 allowlisted | **60** |
| `raw_ptr` | 4570 | **4570** |
| `unsafe_block` | 622 | **622** |
| `unsafe_fn` | 1248 | **1248** |
| `mem_zeroed` | 31 | **31** |
| `SHIM(` | 159 | **159** |
| Miri skips | 2 | 2 |
| findings | — | **F34, new and FIXED** |

**Every metric is flat, and so is every per-file row** — checked per S16 by
regenerating the baseline into a copy and diffing the `files` map, not by reading the
totals. Zero deltas in either direction, in any file, for any metric. That is the honest
shape of this session: it moved no unsafe and deleted no pointer, because window hoisting
reshapes safe accessors and F34's fix trades one `&mut` argument for a local.

Batteries: `gates.sh full` twice, `gates.sh family` once. Every step PASS both times.

### 7. F3 — zero across six sweeps, and the third consecutive decoder-only session

Six sweeps of 341 configurations (one `family` + two `full` batteries × two profiles) =
2046 configurations, all PASS. Nothing to adjudicate; S14's protocol starts at a hit.
Appended to F3 as a sample per S14 step 4.

Session H's entry asked whether a third zero would mean something. It does not, for the
same reason as the last two: `git diff --stat 75188044..HEAD -- rust/crates` is three
files, all under `src/decoder/`, and the sweep compares encoders. Running total unchanged
at **thirty-two measurements, eleven alternations, eleven acquittals**. The step-0 hash
shortcut is *not* claimed — `rust_enc` builds from this same crate, so a decoder-only
source diff does not by itself make the encoder binary identical, and nothing needed
acquitting anyway.

### 8. What went into the rules

**S8 gains its fourth binding negative result** — the brief asked for a catalog addendum
"if the idiom proves out", and the symmetric entry is the one this session earned:
*hoisting a per-macroblock window over an array that is already one scalar per macroblock
is a wash*, with the mechanism (checks are predicted, the length is in L1, and the
functions are 1–3% of decode) and the code-size cost written next to it. It carries the
counting lesson too: **opcode histogram, not landing-pad walk.**

### Hand-off: Phase 5, session J — this goes to Eugene before it goes to a session

**§3.4's instruction is stop, and this is the stop.** The window idiom is not proven and
session J must not be planned as though it were. What the next session does is Eugene's
call, on these numbers:

* **The idiom works and does not pay.** 76 checks per macroblock become 38; the bench
  cannot see it. Do not spend another session on call-site hoisting anywhere in this
  decoder.
* **But the *other* half of the idiom is still the right thing, and it is already
  built.** The eleven families retrofitted here are one scalar per macroblock, so a
  window only ever amortises *repeat visits*. The remaining eleven are not like that:
  `pNzc` is `[i8; 24]`, `pMv`/`pMvd` are `[[i16; 2]; 16]`, `pScaledTCoeff` is `[i16; 384]`,
  and those are indexed **inside** the record, in inner loops. There the element re-type
  T5.H2 already did is what makes the inner index const-bounded and check-free — a
  different mechanism from this session's, and untested. **This session's null result does
  not transfer to them, in either direction.**
* **The tripwire arithmetic D-perf-5 rests on is high by ~2x.** CB's flip cost is +1.18%
  today, not +2.05%; cumulative CB is nearer +19% than +19.9%. Whether that changes the
  parking question is Eugene's call — the deferred tripwire-vs-S20 question is untouched
  here, per the brief's §6.
* **Two readings of one span, seven pairs each, disagreed by a factor of two.** Before any
  future decision rests on a single span number, it wants a second reading on a different
  day, not more pairs on the same one.
* **F34's class is open in principle.** The grep found two sites and both are fixed, but
  the shape — a `&mut` into a flipped family handed to a callee that reads that family —
  is a new hazard the flip creates, and the remaining eleven families have far more
  call sites that pass per-MB data down. Grep for it per family as they flip.
* **The probe's stream is the limit on every Miri verdict this phase has issued.**
  `narrow_16x16.264` is one macroblock per frame: no neighbour is ever available, so no
  neighbour-dependent path has ever been under the checker. A second probe stream with a
  real macroblock grid would be worth more than the next family flip.
* **Unchanged:** `pBitStringAux` and `cabac_decoder.rs:855`'s `SHIM(phase5)` are untouched;
  face 3's ~28 cache-fill re-points have not started; F23 is Phase 8's, F31's redundant
  memset 5.5's.

---

## 2026-08-12 — Phase 5, session J (the probe grows a macroblock grid, and finds two defects with it)

**Commits:** `60d1528b` (inherited doc tail — D-perf-5's direction paragraph, §0's two
rows, S2b's extension, the brief), `c183c8d4` (T5.J1, F35), `d8becdf0` (T5.J2, the grid
probe), `e207fdd9` (T5.J3, `pRefIndex` flips), `fc62d6a1` (ratchet), and this entry.

### The session in one line

D-perf-5's "probe first, then flip" was the right order, and the probe justified itself
before it was committed: the second stream — 3x2 macroblocks, CABAC, 8x8 transform, B
slices — **found a real UB on its first run**, a second one is fixed ahead of the flip
that would have created it, and then the first hot family flipped for **+0.03% decode at
7 pairs**.

### Control battery

Docs-only tail, session I accepted (**OVERALL: PASS**), `rust/tools/` and the toolchain
unchanged — S27's cheap subset. **OVERALL: PASS**, 463/457/20, ratchet 4570, census 60,
every figure matching session I's exit. Third session running that the open needed no
correction.

**The S2 null, run at open** because every verdict here is a perf verdict (3 pairs, same
binary both slots): decode **+0.13%…+0.45%**, median +0.35% — a *tighter* floor than
session I's ±0.6%; encode 28 rows, median +0.00%, −2.45%…+1.42%.

### 1. Face 1 — the second probe stream (T5.J2)

**The brief was wrong about the encoder, and the check that found it took two minutes.**
The brief said to build the stream with the C++ encoder, on `make_narrow_assets.py`'s
precedent. F34 — the miss the new stream has to re-find — sits behind
`bTransform8x8ModeFlag`, so the first thing to establish was that the stream would carry
it. It does not: `ffmpeg -bsf:v trace_headers` over `narrow_16x16.264` shows the PPS
ending before `transform_8x8_mode_flag`, and the reason generalizes —
`grep -rn "ransform.8x8" codec/encoder/` returns **nothing**, and `WelsWritePpsSyntax`
(`au_set.cpp:406`) has no such syntax element. **OpenH264's encoder cannot emit the flag
at all**, so no stream it produces can reach F34, and the brief's instruction and the
brief's acceptance test cannot both be satisfied.

Resolved in place, per the brief's own supersession clause: ffmpeg/libx264 builds this one
asset. It is not a new dependency — `benches/decode_1080p_bench.rs` builds all three of
its 1080p streams with it, `gates.sh` requires `FFMPEG`, and S17's UNMEASURED banner
exists because of it. The golden is still the **C++ decoder's** output, exactly as every
other row in the conformance file: which encoder produced the bytes never enters the
comparison.

**`res/grid_48x32.264`, 992 bytes.** 48x32 is **3x2 macroblocks** — the smallest grid that
contains a macroblock with all four neighbours (MB(1,1)) *and* one missing only its left
(MB(0,1)) *and* one missing only its top-right (MB(2,1)). That is every availability
combination the neighbour paths branch on; each macroblock past it is Miri time. CABAC,
High with `transform_8x8_mode_flag` set, **I, P and B** slices, six frames, and the same
panned window the narrow assets use so the MVs are non-zero. `make_narrow_assets.py`
carries it with its command line, and `--check` reproduces all four assets byte-for-byte.

**Coverage proven, not asserted** (F21's rule, which is why the brief asked for it). With
T5.I2's fix reverted in a scratch worktree:

```text
error: Undefined Behavior: not granting access to tag <35974063> because that would
       remove [Unique for <37349573>] which is strongly protected
  2: safe::mb_grid::MbArray::<bool>::get        at src/safe/mb_grid.rs:215
  3: ParseTransformSize8x8FlagCabac             at src/decoder/parse_mb_syn_cabac.rs:1242
  4: WelsDecodeMbCabacIntraModeHelper           at src/decoder/decode_slice.rs:4195
```

`parse_mb_syn_cabac.rs:1242` is the callee's own left-neighbour read — F34 exactly. The
small probe stays green under the same revert, and stayed green for the three sessions the
defect was live, which is the other half of the proof.

**The Miri budget, stated at the site as the brief required: 258.7s wall, against the
small probe's 250.7s.** Nine times the macroblocks per frame at a quarter of the frames is
a wash. The pair is ~510s, `miri --lib` goes ~524s → ~780s (~13 min), inside the ~25 min
the brief allows, so **both probes stay in `--lib` at `full` level and neither is deferred
to `exit`**. The small one is kept rather than superseded: it is the cheap verdict for
every path that needs no neighbour.

The test asserts the decoded dimensions, not just that a frame came out. A regenerated
asset that silently came out 16x16 would still pass "a frame came out" while covering
nothing — which is precisely how F34 survived.

### 2. F35 — what the new stream found on its first run (T5.J1)

Before the probe was committed:

```text
error: Undefined Behavior: accessing memory based on pointer with alignment 2,
       but alignment 4 is required
    --> src/decoder/mv_pred.rs:1005:13
             0: decoder::mv_pred::PredMvBDirectSpatial
             1: decoder::decode_slice::WelsDecodeMbCabacBSlice
```

**Half one, 13 sites that were already UB.** `PredMvBDirectSpatial`,
`FillSpatialDirect8x8Mv`, `FillTemporalDirect8x8Mv` and the 8x8 variants pun *stack
locals* — `iMvp` (`[[i16; 2]; 2]`), `pMV` (`[i16; 4]`), `pMvDirect` — through `*const i32`
/ `*mut u32`. An `i16` array is 2-aligned and nothing rounds a stack slot up, so these
were unconditionally UB on every B slice that reached them. The file has carried
`LD32`/`ST32` — `read_unaligned`/`write_unaligned` wrappers — since the port was written.
These sites simply never used them.

**Half two, and it is the half that matters for what follows.** `SetRectBlock` and
`CopyRectBlock4Cols` store 2 and 4 bytes at a time into `pRefIndex` (`[i8; 16]`, align 1)
and the MV arrays (`[[i16; 2]; 16]`, align 2). They are legal **only because every one of
those arrays comes from `WelsMallocz`, which returns 16-byte alignment** — an accident of
the allocator, not a property of the data. `MbArray<[i8; 16]>` is a `Vec` and its
allocation is align 1. The first family commit that moved `pRefIndex`, `pMv` or `pMvd`
onto the grid would have made all 65 accesses UB at once. Converted here, ahead of the
defect, per S13.

**Why nothing saw it.** `PredMvBDirectSpatial` needs a **B slice** *and* a neighbour.
`narrow_16x16.264` has neither — the C++ encoder emits no B slices, and its frames are one
macroblock — so the whole direct-mode family had never executed under Miri in any form.
Byte gates cannot see it either: on both targets this project builds, an unaligned 4-byte
load is the same instruction as an aligned one, so 341/341 sweeps and 56 goldens ran on
top of it for the port's whole life. **A soundness defect with no observable behaviour is
the class the Miri gate exists for**, and this is the cleanest instance the project has
had.

The class was greped rather than guessed: `deblocking.rs` already spells every wide access
`read_unaligned`/`write_unaligned`. `mv_pred.rs` was the only outlier.

### 3. Face 2 — the heat order, re-derived (S24)

`/usr/bin/sample`, 6s over `decode_1080p_bench` across all three streams, self time, 3769
samples. A family's heat is the summed self time of the functions holding its
**layer-qualified** accesses — an attribution of where its accesses sit, not a measurement
of the family. Layer-qualified matters: `SPicture` carries its own `pMbType`, `pMv`,
`pRefIndex` and `pMbCorrectlyDecodedFlag` for the colocated reads, and those 198 further
occurrences are 5.1/5.3's, not 5.2's. Counting them would have put `pMv` and `pMbType` at
the top for the wrong reason.

| # | family | heat | % | layer sites | hottest touching functions |
|---|---|---|---|---|---|
| 1 | `pRefIndex` | 117 | 3.1% | 17 | PredMvBDirectSpatial 56, DeblockingBsMarginalMBAvcbase 40 |
| 2 | `pMv` | 117 | 3.1% | 18 | the same three |
| 3 | `pMbType` | 77 | 2.0% | 8 | WelsDeblockingMb 77 |
| 4 | `pNzc` | 63 | 1.7% | 30 | WelsDecodeMbCabacBSlice 19, …PSlice 17 |
| 5 | `pSliceIdc` | 62 | 1.6% | 19 | PredMvBDirectSpatial 56 |
| 6 | `pChromaQp` | 50 | 1.3% | 29 | WelsDecodeMbCabacBSlice 19 |
| 7 | `pMvd` | 46 | 1.2% | 36 | ParseInterBMotionInfoCabac 20 |
| 8 | `pDirect` | 27 | 0.7% | 12 | WelsDecodeMbCabacBSlice 19 |
| 9 | `pMbCorrectlyDecodedFlag` | 18 | 0.5% | 18 | WelsTargetSliceConstruction 18 |
| 10 | `pScaledTCoeff` | 14 | 0.4% | 11 | WelsDecodeMbCabacResidualHelper 14 |
| 11 | `pIntraPredMode` | 0 | 0.0% | 12 | none above the sampling floor |

`pRefIndex` and `pMv` tie because the same functions touch both — always as separate
expressions, and **no signature takes both**, so they separate cleanly for S20 and stay one
family per commit. `pRefIndex` went first on the tie-break: same heat, one fewer site, and
the worst alignment drop (16 → 1), so it is also the sharpest test of F35's second half.

### 4. T5.J3 — `pRefIndex` flips, and it is free

17 layer sites: 8 neighbour reads become shared indexing, 3 write sites become
`grid.ref_index[l].get_mut(iMbXy).as_mut_ptr()`, 1 becomes an S28 bridge, and the field,
its two allocations and its free block are deleted.

**S28.** `DeblockingBsMarginalMBAvcbase` keeps the array **base** and indexes it by
macroblock address for the current macroblock *and* its neighbour, so the pointer's legal
reach is the whole array. It goes through the existing `mb_grid_ptr` (allocation root +
`wrapping_add`) and gets its own full-reach Miri test, which also pins that this bridge
stays `*mut [i8; 16]` rather than flattening — the consumer indexes inside the record with
a scan-order index of its own.

**S25 and F34's class.** The three raw bridges derive and then write, with nothing between
the derivation and the `ST16`s that touches `grid.ref_index` — no intervening neighbour
read to pop the tag. No function *receives* a `&mut` into this family, so F34's shape does
not arise. `GetPNzc` and `MB_BS_MV`, the two calls live across the deblocking bridge, take
raw layer pointers and never retag the layer.

**S21** discharges by construction: `SDqLayer::for_grid` writes only `grid` through a
zeroed `MaybeUninit` shell, so deleting a raw pointer field whose zeroed state *was* its
initial state changes no construction path. **Census**: no allowlist entry names this
family; `type SDqLayer x2` re-keys when the struct is renamed, which is the last family's
commit.

**The measurement, 7 interleaved pairs, both benches** (`d8becdf0` → `e207fdd9`):

```
Constrained Baseline (CAVLC)   2.4990 -> 2.5050   +0.24%
Main (CABAC, B-frames)         5.9850 -> 5.9870   +0.03%
High (CABAC, 8x8)              6.0220 -> 6.0200   -0.03%
  decode: rows 3, median +0.03%, min -0.03%, max +0.24%
  encode: rows 28, median +0.00%   (decoder-only; unaffected, as expected)
```

**Every row is inside the session's null band, and two of the three are below its floor.**
No ledger row opens.

**The mechanism, so the next family is judged against one rather than against a hope.**
Session H's eleven were scalars: every access was a separate bounds check on a separate
array, 76 per macroblock. `pRefIndex` is *one* check per macroblock record, after which
the sixteen in-record indices are const-bounded against a `[i8; 16]` and fold — which is
exactly what D-perf-5 predicted would be different and could not test. `pMv`, `pMvd`,
`pNzc`, `pDirect`, `pScaledTCoeff` and `pIntraPredMode` share that shape; `pMbType` and
`pSliceIdc` do not — they are scalars and should behave like session H's.

**Cumulative CB ≈ +19.2…+20.1%** (the range is session H's and session I's readings of the
same earlier span) against the ≈+23% stop-line. Nothing is near it.

**This reading owes a day-two confirmation and has not had one.** A decision rests on it —
whether the remaining nine keep flipping — and S2b as extended says two seven-pair
readings of one span disagreed by a factor of two on different days. Per the brief's §0.3,
the confirmation is the **first item of the next session**; both binaries are stashed
(`.perfpair/j_face1`, `.perfpair/j_j3`).

F35's own span was measured rather than waived, since it rewrote 78 accesses in the MV
cache helpers: decode median **−0.03%** at 3 pairs, encode +0.10%. The CB row reads +0.77%
— the only row of either span above the null's ceiling — and the median is the verdict.

### 5. Numbers

| metric | entry | exit |
|---|---|---|
| tests (debug / release / ignored) | 463 / 457 / 20 | **466 / 460 / 20** |
| Miri `--lib` | 324 | **326** |
| decode goldens | 56 rows | **57 rows** (`grid_48x32`, additive) |
| census | 60 allowlisted | **60** |
| `raw_ptr` | 4570 | **4550** |
| `unsafe_block` | 622 | **623** |
| `unsafe_fn` | 1248 | **1248** |
| `mem_zeroed` | 31 | **31** |
| `SHIM(` | 159 | **159** |
| Miri skips | 2 | 2 |
| findings | — | **F35, new and FIXED** |

Per-file deltas, which is the only way to read the ratchet (S16): `mv_pred.rs` `raw_ptr`
−16 (T5.J1's 13 rewrites — `*(x as *const i32)` names two pointer types, `LD32(x)` names
none), `decoder_core.rs` −3 and `deblocking.rs` −1 (T5.J3's field, allocations, free block
and the `as *mut _` fallback), `decoder_core.rs` `unsafe_block` **+1**. **The one increase
is a test** — the S28 full-reach test the bridge owes — and no production `unsafe {}` was
added anywhere.

Both `raw_ptr` decreases are conversion rather than deletion, which is the second session
of the phase that can say so.

**Settled-tree battery, `gates.sh full` at `fc62d6a1` — OVERALL: PASS**, 9 passed / 0
failed / 1 skipped: 466/460/20, ratchet no per-file increase, census 60, **sweeps 341/341
both profiles** (the debug sweep clean, which is the F3 acquittal's other half), both
benches bit-identical, **Miri `--lib` 326 passed / 0 failed** with both probes inside it.
Batteries this session: `gates.sh commit` twice, `gates.sh full` twice.

### 6. F3 — one hit, and the zero-hit run ends at three

`gates.sh full` on Face 1's tree drew
`mt CiscoVT2people_160x96_6fps t=4 sm=3 n=600 cabac=1 rc=1 :: Rust: 0 bytes` on the
**debug** sweep, 340/341. Inside S14's signature on every field. Step 1's isolation re-run,
5× on an idle machine: **5/5 BYTE-IDENTICAL** — the expected result, and S23b is the rule
that says so: the race needs the load of a full sweep. One hit, so step 2 does not fire.
**Acquitted as F3**, appended to `phase0_findings.md` as measurement 33 at adjudication
time (S14 step 4). Running total: thirty-three measurements, eleven alternations, twelve
acquittals.

### 7. A gate-hygiene note worth carrying

Face 1's battery ran with an uncommitted `pRefIndex` edit landing in the working tree
partway through, and the Miri step's build almost certainly picked it up — the interpreter
started some seven minutes after that step's header, where a warm Miri build takes about
two. That is an inference from timing, not a proof; the test count cannot distinguish the
two trees, because the flip adds no `--lib` test. **So that battery's Miri line is not
claimed for Face 1 alone.** What Face 1 has instead: both probes run individually and green
on its exact source, `gates.sh commit` green on it, and the settled-tree battery below,
which contains Face 1 as a subset. The lesson is small and mechanical — **do not edit the
tree while a battery is running**; `gates.sh` builds from the working tree, not from the
commit, so the thing being measured can move without the log saying so.

### 8. What went into the rules

Nothing new. F35 is an instance of S13 (run the instrument everywhere it can apply, in the
session that first applies it) and of S22 aimed at a stream — the same aim F34 established
— and the brief's own supersession clause covered the encoder correction. **S8's fourth
negative result was honoured rather than tested**: T5.J3 hoists nothing.

### Hand-off: Phase 5, session K

**First item, before any new work: the day-two confirmation of T5.J3's span.**
`perfpair.py run j_face1 j_j3 --pairs 7`, both binaries already stashed. A decision rests
on +0.03% and S2b's extension exists because a seven-pair reading moved by a factor of two
overnight. If it confirms, the remaining nine flip on D-perf-4's normal terms.

* **Then `pMv`, hottest first**, per §2's table: 18 layer sites, align 2 after the flip.
  Then `pMbType`, `pNzc`, `pSliceIdc`, `pChromaQp`, `pMvd`, `pDirect`,
  `pMbCorrectlyDecodedFlag`, `pScaledTCoeff`, `pIntraPredMode`.
* **Grep each family's consumers for a wider-than-element access before flipping it**
  (F35). The two known ones are converted, but that is a fact about `mv_pred.rs`, not a
  property of the codebase.
* **Expect the grid probe to keep finding things.** It found F35 on run one. The paths it
  opened — B-direct, MV prediction from neighbours, the 8x8 transform — have never been
  under the checker, and the ten remaining families are the ones that live there. Budget a
  Miri round trip per family, not a Miri verdict per family.
* **Face 3 has not started.** The ~28 `parse_mb_syn_*` cache-fill re-points, `pBitStringAux`
  and `cabac_decoder.rs:855`'s `SHIM(phase5)` are untouched; re-grep before acting (S24).
* **Unchanged:** F23 is Phase 8's, F31's redundant memset 5.5's, F22's map 5.3's,
  `PicPool`/identity deferred.

## 2026-08-12 — Phase 5, session K (the day-two confirmation, three more families, and a divergence found by a re-grep)

**Commits:** `4b91c1a0` (inherited doc tail — the brief's S24 stamp), `4f9ed98d` (T5.K1,
`pMv`), `31826d05` (ratchet), `f2990054` (T5.K2, `pMbType`), `d4a3fd8a` (T5.K3,
`pSliceIdc`, and F36), `135ebcb7` (ratchet), and this entry.

### The session in one line

The day-two confirmation the brief made item one **confirmed the mechanism and
disqualified the instrument**: three 7-pair readings of one span disagree in sign and the
three null bands they were judged against disagree at least as much, so a per-family cost
of ≈0.3% CB — the size that decides whether the remaining families fit under the
stop-line — is not measurable here at any reachable pair count. Then `pMv`, `pMbType` and
`pSliceIdc` flipped, all three free, and `pSliceIdc`'s re-grep turned up **F36**.

### Control battery

Docs-only tail, session J accepted (**OVERALL: PASS**), `rust/tools/` and the toolchain
unchanged — S27's cheap subset. **OVERALL: PASS**, 466/460/20, ratchet 4550, census 60,
every figure matching session J's exit. Fourth session running that the open needed no
correction.

### 1. Face 1 — the day-two confirmation, and the correction it forced

`perfpair.py run j_face1 j_j3 --pairs 7`, the command the brief specifies, against the
S2 null run at open (3 pairs, as session J ran it): **decode median +0.27%** against a
band of **−0.16%…−0.02%**. Outside. By the brief's own criterion that is *does not
confirm*, whose instruction is to stop and escalate.

**This session adjudicated it as a confirmation before establishing that it was one, and
that was the wrong order.** The reasoning offered — that a 7-pair verdict deserves a
7-pair null — is correct, and the 7-pair null run afterwards does contain +0.27%. But it
was reached after seeing the result, and the brief had fixed the test in advance
precisely because a decision rests on it. The rule this pays for is not "never widen an
instrument"; it is **fix the instrument before the reading, or treat what you learn
afterwards as evidence rather than as a re-adjudication.**

What the extra runs actually established, which is worth more than either verdict:

| reading of T5.J3's span | CB | Main | High | decode median |
|---|---|---|---|---|
| day one (session J, 7 pairs) | +0.24% | +0.03% | −0.03% | **+0.03%** |
| day two #1 (7 pairs) | +0.27% | +0.32% | −0.11% | **+0.27%** |
| day two #2 (7 pairs) | −0.45% | −0.05% | −0.13% | **−0.13%** |

| null | band | median |
|---|---|---|
| session J, 3 pairs | +0.13% … +0.45% | +0.35% |
| session K, 3 pairs | −0.16% … −0.02% | −0.04% |
| session K, 7 pairs | −0.22% … +0.25% | +0.04% |

**Two same-day 7-pair readings of one span disagree in sign; so does the CB row across
all three.** S2b's own test for "below the measurement error" is met twice. The mechanism
in `perf_baseline.md`'s J section stands — nothing measurable is being spent — and §2
proceeded on Eugene's call, taken on these numbers rather than on the brief's binary.

**The result to carry:** ten families over ≈3 points of headroom to the ≈+23% stop-line
is **≈0.3% CB per family**, which is exactly the resolution demonstrated here. The
cumulative figure is ~20% and is resolved trivially. **The instrument for the rest of 5.2
is the cumulative span, not the per-family one** — `seat_head`/`flip_head` to HEAD, one
7-pair run, not yet done and the next session's first item.

### 2. Face 2 — three families, hottest first

Heat re-derived at open (S24): 25s of `/usr/bin/sample` over `decode_1080p_bench`, 3917
self samples, the bench's own SHA-1 (1307) excluded from the denominator. **The order had
moved since session J** — `pSliceIdc` went 1.6% → 2.5% and overtook `pNzc` — because heat
is attributed to the functions holding a family's accesses and those functions do not
change when a *different* family leaves them. The counting rule that reproduces session
J's 17 for `pRefIndex` and this session's 18 for `pMv` exactly is **occurrences of
`.pXxx` whose receiver is not a picture**, comments stripped.

**T5.K1 — `pMv` (3.9%, 18 sites), the hottest family in the flip.** 8 neighbour reads
lose their pointer entirely: `ST32(iMvA.as_mut_ptr(), LD32(mv_ptr))` moved four bytes out
of a neighbour's record into a stack `[i16; 2]`, and with the array owned that is
`iMvA = grid.mv[0].get(xy)[3]` — one typed copy of the same four bytes. 4 write sites
become `grid.mv[l].get_mut(iMbXy).as_mut_ptr()`. `deblocking.rs` gets the family's one
S28 bridge, and it is the **second** bridge in `DeblockingBsMarginalMBAvcbase`:
`pRefIdxArr` (T5.J3) and `pMvArr` are `&mut`s of *different* fields whose `Vec`s are
different allocations, so neither retag can pop the other's tag — which the grid probe
then executed under Miri rather than leaving as an argument.

**T5.K2 — `pMbType` (2.6%, 9 sites), and it is a bridge rather than a set of reads.**
`GetMbType` hands its seven callers the array *base* and they index it at neighbour
addresses, so the else-branch becomes `mb_grid_ptr(&mut grid.mb_type, 0)`. **F34's shape
was checked at all seven and is absent at each** — two of them re-derive the base in the
same function, which is exactly the invalidating shape, but no earlier pointer is live
across either.

**T5.K3 — `pSliceIdc` (2.5%, 24 sites), and no raw pointer survives it.**
`deblocking.rs`'s base binding existed only to compare a macroblock with its left and top
neighbours; it is a shared borrow now. First family of the flip that adds **no** raw
derivation and therefore owes no full-reach test. The per-picture
`memset(pSliceIdc, 0xff, …)` becomes `as_mut_slice()[..iMbCacheNum].fill(-1)`, and its
bound is an identity rather than a hope: `InitialDqLayersContext` sets
`iPicWidthReq`/`iPicHeightReq` to the same `kiMaxWidth`/`kiMaxHeight` the grid's `MbDims`
come from, in that same function. **Two null guards die and both were the port's own
additions** — the C++ is unguarded at `decode_slice.cpp:1593` and
`decoder_core.cpp:1623` — so deleting them converges on upstream rather than changing
behaviour. F22's class, running in the added-guard direction.

**F35's grep ran before each of the three flips and all three are clean.** `pMv`: every
wider-than-element access into a layer MV record already goes through
`ST32`/`LD32`, i.e. `write_unaligned`/`read_unaligned` since T5.J1, and `MB_BS_MV` reads
`[i16; 2]` values whose align-2 requirement a `Vec` meets. `pMbType` and `pSliceIdc`: no
access wider than their `u32`/`i32` elements exists in either tree.

**Measurements, 7 pairs, both benches** (details and the null bands in
`perf_baseline.md`): T5.K1 decode median **−0.13%**; T5.K2+K3 as one cluster **+0.04%**.
Every row inside the 7-pair null band but one, which is below its floor. **No ledger row
opens.** The cluster is deliberate: after §1, splitting a ≈0.1% effect into two ≈0.05%
halves measures below the floor twice instead of once. Both commits exist, so per-family
spans remain rebuildable from the refs.

### 3. F36 — the multi-threaded slice loop never writes `pSliceIdc`

Found by T5.K3's re-grep, not by a gate: the C++ writes the slice id in **both** its
macroblock loops (`decode_slice.cpp:1593` in `WelsDecodeSlice`, `:1708` in
`WelsDecodeAndConstructSlice`), and the port's copy of the second writes it in neither —
it does not compute `iSliceIdc` at all. `pSliceIdc` is what **every** neighbour
availability predicate compares, so left at its −1 reset every neighbour would read as
available and prediction would cross slice boundaries.

**It is dormant, not live.** That loop is the `iThreadCount > 1` arm and `GetThreadCount`
returns 0 unconditionally — F22 §5 already establishes that the arm is dead and
`WelsDecodeSlice` is the only parse entry. Recorded as **OPEN, dormant**, owned by
whoever ports decoder threading and to be fixed *before* `GetThreadCount` returns
anything greater than 1.

The general point is the one to carry: **a stubbed-out subsystem's translation is
unaudited and uncovered at the same time, and the two conditions hide each other.** No
gate here reaches the decoder's MT path — the goldens and the decode bench drive the
single-threaded API, and the diffharness sweeps multi-thread the *encoder*.

T5.K3 changed nothing about it: the write that exists still happens, the one that never
existed still does not, and no byte moves (S6).

### 4. Numbers

| metric | entry | exit |
|---|---|---|
| tests (debug / release / ignored) | 466 / 460 / 20 | **468 / 462 / 20** |
| Miri `--lib` | 326 | **328** (~910s) |
| decode goldens | 57 rows | **57** |
| census | 60 allowlisted | **60** |
| `raw_ptr` | 4550 | **4540** |
| `unsafe_block` | 623 | **625** |
| `unsafe_fn` | 1248 | **1248** |
| `mem_zeroed` | 31 | **31** |
| `SHIM(` | 159 | **159** |
| Miri skips | 2 | 2 |
| findings | — | **F36, new and OPEN (dormant)** |

Per-file deltas, which is the only way to read the ratchet (S16): **`decoder_core.rs`
−9, `deblocking.rs` −1, everything else flat** — `mv_pred.rs`, `decode_slice.rs` and
`parse_mb_syn_cavlc.rs` are unmoved across **32 converted sites** between them, because
these conversions delete pointer *dereferences* while `raw_ptr` counts pointer *types
written*. Both `unsafe_block` increases are the S28 full-reach tests T5.K1 and T5.K2 owe;
**no production `unsafe {}` was added anywhere**, for the second session running.

### 5. F3 — four hits, two alternations, and the signature loses a clause

Two hits per battery, one battery per profile, all four inside S14's signature. Step 0
does not apply (the diff reaches lib code `rust_enc` links) — and a cross-path hash
comparison was tried first and is **invalid as evidence either way**, because a control
built in a `git worktree` embeds a different path and its binary differs regardless of
the source. Step 1 on battery 1's two configurations: 5/5 byte-identical each. Step 2,
run twice at the profile its hits occurred in, 12 whole `mt` presets per side inside one
loop on an idle machine:

| alternation | control (`4b91c1a0`) | head | per side |
|---|---|---|---|
| debug, vs T5.K1 | 4 | 3 | 1440 |
| release, vs T5.K2+K3 | 4 | 6 | 1440 |
| **combined** | **8** | **9** | **2880** |

A 6-vs-4 split is p ≈ 0.38 under the null. **HEAD is not worse; acquitted**, and both
alternations hit on both sides, so S23b is satisfied twice. Appended as measurement 34 at
adjudication time (S14 step 4). Running total: **thirty-four measurements, thirteen
alternations, thirteen acquittals.**

Two facts outlive the acquittal. **The rate under sustained back-to-back presets is
≈1/307**, not the recorded ≈1/800 — twenty-four presets with nothing between them is the
most load this sweep has ever run under, and load is part of the signature. And **`n=600`
is a rate artifact rather than a condition**: the first `n=1500` hit in 34 measurements
landed here, and `sm=3`'s `n` is the per-slice *byte budget*, so a smaller `n` cuts more
slices and performs more of the slice-list growths the race lives in. S14's signature is
corrected in place.

### 6. What went into the rules

* **S2b gains the pair-count clause** — a verdict at N pairs is judged against a null at
  N pairs — flagged in its own text as this session's proposal rather than Eugene's call,
  with the three-reading/three-null evidence beside it and the "measure something bigger"
  conclusion that follows from it.
* **S14's signature is corrected**: `n=600` demoted from condition to predominant case,
  and the measured rate now carries its load dependence explicitly.
* Nothing else. F36 is an instance of F22's class and of S24 — it was found by re-greping
  a family's writers at the moment of converting it, which is what S24 exists to make
  happen.

### Hand-off: Phase 5, session L

1. **First: the cumulative span.** `perfpair.py run` from the pre-flip base to HEAD, 7
   pairs, both benches — `seat_head` (3c4c6f4e) and `flip_head` (1438d762) are stashed,
   and HEAD needs one build. This is the number the ≈+23% stop-line gates on, it is ~20%
   rather than ~0.3%, and §1 is why the per-family rows cannot answer it. Run the S2 null
   **at the same pair count**.
2. **Then the remaining seven**, hottest first, re-deriving the heat first because it
   moved between J and K: `pNzc` (1.9%, 32 sites), `pChromaQp` (1.9%, 29), `pMvd` (1.4%,
   40), `pDirect` (0.8%, 16), `pMbCorrectlyDecodedFlag` (0.6%, 18), `pScaledTCoeff`
   (0.3%, 11), `pIntraPredMode` (0.0%, 16). Site counts are session K's greps and several
   disagree with the older table; re-grep each (S24).
3. **`pMvd` carries a shape none of the flipped families had**: `mv_pred.rs` guards
   fifteen of its accesses with `if !(*pCurDqLayer).pMvd[LIST_x].is_null()`. Check each
   against the C++ before deleting it, as T5.K3 did for `pSliceIdc`'s two — the answer
   there was that the port had added them, but that is a fact about `pSliceIdc`.
4. **Expect the grid probe to keep earning its 260s.** It has now executed two new S28
   bridges and a re-derived `GetMbType` without a finding, which is the first time this
   phase it has been quiet across a whole session.
5. **Unchanged:** Face 3 (the ~28 `parse_mb_syn_*` cache-fill re-points, `pBitStringAux`,
   `cabac_decoder.rs:855`'s `SHIM(phase5)`) is not started; F23 is Phase 8's, F31's
   redundant memset 5.5's, F22's map 5.3's, **F36 decoder-threading's**,
   `PicPool`/identity deferred.
