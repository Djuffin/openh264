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

### Next session's first action

**Phase 2 T3 — `decoder/get_intra_predictor.rs`**, the 44-kernel family and the phase's
perf worst case, taken early on purpose. Two things to carry in:

- **Its shims are the first that cannot be derived from the signature.** All 44 read
  the `-1` column and the `-stride` row, so each shim must build its slice from
  `pPred.sub(stride + 1)` and its `# Safety` contract must state that the block has at
  least one valid row above and one valid column left. That contract is identical for
  the whole file, which is what makes 44 kernels tractable in one family.
- **Do not unify the same-named 3-arg cousins in `common/intra_pred_common.rs`.**
  Different functions, different arity, T5's job.

Then T4 (`mc.rs`) and so on down §4's list. There is no longer a competing "if Phase 0
is picked up instead" branch: T5c and T5e landed this session, and T7 (fuzz) is deferred
by direction — its absence is now printed as a SKIP on every `gates.sh` run so it cannot
quietly stop being a decision.
