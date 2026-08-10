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
