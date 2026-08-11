# Safety refactor — session log

One entry per working session on [`safety_refactor_plan.md`](safety_refactor_plan.md).
Newest last. Each entry records what landed, the gate results (control vs final), what
was found but not fixed, and the next session's first action — so a session can resume
from this file and the plan's Progress appendix alone.

---

## 2026-08-07 — Phase 0, session A (T1–T5b)

**Goal:** Phase 0 tasks T1–T8 per `prompts/phase0.md`. Got through T1–T4, T5a and T5b;
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

**Goal:** Phase 1 per `prompts/phase1.md` — build the safe vocabulary types in
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

**Goal:** `prompts/phase2.md`'s session-A split — the preconditions (Phase 0's T6,
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
`prompts/phase2.md` §T5 and the continuation brief. Carry-ins unchanged: the `Four` SAD
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
`prompts/phase2_finish.md` §3 — the brief's order is binding, T9 never compresses.
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
`prompts/phase4a.md`, and this entry.

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
hoisted from `prompts/phase2_continue.md` §2 — briefs from here on cite it instead of
copying rules forward), and **`prompts/phase4a.md`**. Both Phase 2 briefs are stamped
superseded-historical.

### Next session's first action

**Phase 4a — read [`prompts/phase4a.md`](prompts/phase4a.md)**, then plan §0 and §7.6.
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

**Goal:** `prompts/phase4a.md` — de-virtualize kernel dispatch, then run the recovery
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
`prompts/phase3.md`.

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

Phase 3, per [`prompts/phase3.md`](prompts/phase3.md): read F4/F5/F7, F2, and F13's
`InitBits` site, then **write the malformed-stream error-code parity test against the
unconverted reader** before touching `bit_stream.rs`. That test is the phase's real
gate and it is worthless if written after the conversion it is meant to judge.

---

## 2026-08-10 — Phase 3, session A (T3.0 + T3.1a)

**Goal:** Phase 3 per [`prompts/phase3.md`](prompts/phase3.md), session 1 = T3.0 + T3.1.
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

**Goal:** [`prompts/phase3_session_b.md`](prompts/phase3_session_b.md) — finish T3.1,
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

**Goal:** [`prompts/phase3_session_c.md`](prompts/phase3_session_c.md) — (1) make
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

**Goal:** [`prompts/phase3_session_d.md`](prompts/phase3_session_d.md) — seam T3.3
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

**Goal:** [`prompts/phase3_session_e.md`](prompts/phase3_session_e.md) — seam T3.4 in
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
