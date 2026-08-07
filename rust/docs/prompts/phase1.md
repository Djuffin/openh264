# Session prompt — Safety refactor, Phase 1: the safe vocabulary types

You are executing **Phase 1** of `rust/docs/safety_refactor_plan.md`: build the safe
vocabulary types in a new `src/safe/` module — `PaddedPlane`/plane cursors, the detached
bit cursors `BsCursor`/`BsWriter`, the `Pool`/handle arena, the `MbGrid` addressing
skeleton, and minimal error plumbing — **fully unit- and differential-tested, Miri-clean,
and wired into nothing**. The codec does not change in this phase; the only codec-adjacent
edit permitted is the `pub mod safe;` line in `lib.rs`.

This prompt reflects repo state as of `53e211f7`. Where it disagrees with the plan, the
prompt is newer; where *reality* disagrees with either, reality wins — and you update the
plan (that duty is standing policy, see the log's "Discrepancies" section for the pattern).

**Why this phase exists:** every later phase (2, 3, 5, 6) converts codec code *onto* these
types. An API-shape mistake here is cheap now and expensive in Phase 5. So the bar is:
exact C semantics (proven by differential tests against the existing unsafe code), zero
unsafe, zero speculation (every public item must have a named consumer in the plan or a
differential test justifying its shape — no "might be useful" methods).

---

## 1. Read first, in this order

1. `rust/docs/safety_refactor_plan.md` — at minimum §2.1 (principles), §2.2.1–2.2.6
   (the type contracts you are implementing), §3 P4–P7/P11 (edge semantics the types must
   carry), §7 (gates), §Phase 1, and the `## Progress` appendix (current state).
2. `rust/docs/safety_refactor_log.md` — the session narrative; its gate table is your
   expected-numbers reference; its "Discrepancies" list is the calibration for how much to
   trust written line numbers.
3. `rust/docs/phase0_findings.md` — F1, F2, F3 in full. All three bear on this phase
   (§3 below).
4. `rust/docs/prompts/phase0.md` §3 "Ground rules" and §6 "Ending a session" — the
   working agreements (commit style, one-unit-per-commit, revert-first, clean session
   exits) carry over verbatim.
5. The code you will mirror (skim now, inventory precisely in T2/T3):
   `decoder/bit_stream.rs`, `decoder/dec_golomb.rs` (reader), `encoder/vlc_encoder.rs`
   (writer, the canonical copy per F2), `common/wels_common_defs.rs:30-46`
   (`SBitStringAux`), `decoder/pic_queue.rs:177-330` (`AllocPicture` — the padded-plane
   layout you are encoding), `decoder/picture.rs`.

## 2. Preconditions — check before any Phase 1 work

The plan's Progress appendix gates Phase 1 on Phase 0 being **complete**: both profiles
green on the full battery, ratchet baseline committed (T6), counts strictly below
session-start. As of `53e211f7`, **T5c, T5e, T6 and T7 are unchecked.**

- If they are still unchecked when you start: execute `prompts/phase0.md` from the first
  unchecked task until Phase 0's exit gate is met, *then* return here. Critical
  correction the log recorded against that brief: **`GetThreadCount` simplifies to a
  literal `0`, not `1`** — `api/codec_api.rs:1831` branches on `<= 0` and returning `1`
  changes the decoding timestamp. The T6 ratchet must count `unsafe (extern "C" )?fn`
  (the naive pattern missed all 113 deleted stubs). T7 needs
  `rustup toolchain install nightly` and `cargo install cargo-fuzz`, which were absent
  from this machine.
- If Phase 0 is checked off: run the full battery as your control (T1 below) and proceed.

Phase 1 also wants nightly for **Miri** (`rustup component add miri --toolchain nightly`)
— install alongside the T7 toolchain work if it isn't present.

## 3. State you inherit (read once, believe)

- **Gate battery** (until `gates.sh` exists; use it if T6 landed):
  `cargo test` and `cargo test --release` in `rust/crates/openh264-rs/` — expect
  **294 passed / 0 failed / 20 ignored** each;
  `bash rust/tools/diffharness/sweep.sh st mt def` under `RUST_ENC_PROFILE=debug` **and**
  `=release` — expect **341/341** each (~2m14s debug, ~21s release);
  benches: `decode_1080p_bench` 60/60 hashes match; `c_vs_rust_bench` **only means
  anything with `FFMPEG` set and `BENCH_REQUIRE_FFMPEG=1`** — without it, it measures
  frame-skipping, per `perf_baseline.md`.
- **F3 retry rule:** a release `sweep.sh mt` failure confined to `t=4 sm=3` is a known
  pre-existing nondeterminism (~1 in 400–1000): re-run before treating as regression. A
  failure anywhere else is real immediately. Phase 1 touches no codec code, so *any*
  non-F3 sweep change means you broke the harness or the tree — stop and look.
- **F1's lesson is Phase 1's design brief in miniature:** the release segfault was a
  16-byte stack array written 32 bytes deep through `from_raw_parts_mut` — the size
  relationship lived only in raw-pointer reinterpretations. Your types exist to make that
  class unwritable. When you design an API here, ask "could two call sites disagree about
  a size through this API?" — if yes, the API is wrong.
- **F2 constrains `BsWriter`:** there are **four** writer copies with differing guards,
  masking, and overflow behavior; they agree on in-contract inputs.
  `encoder/vlc_encoder.rs:367` is the canonical reference (matches C++ `bit_stream.h`).
  Phase 1's `BsWriter` implements **canonical semantics** and differential-tests against
  the canonical copy; the guard-semantics decision for the other three copies belongs to
  Phase 3.2 and must not be made implicitly here. Say so in `BsWriter`'s doc comment.
- The e2e `#[ignore]` set is **20** (not 13 — the plan was corrected); it must not change.
- The C++-vs-Rust perf numbers are scalar-vs-scalar; the conformance harness silently
  drops frames on decoder errors (frame counts before hashes, always).

## 4. Ground rules

- **Codec untouched.** The full diff of this phase outside `src/safe/`, `tests/`, and
  docs is exactly one line: `pub mod safe;` in `lib.rs`. No "while I'm here" edits
  anywhere else. Every existing test result, sweep count, hash, and the ratchet baseline
  stay bit-for-bit identical.
- **`#![forbid(unsafe_code)]`** as an inner attribute at the top of **every** file under
  `src/safe/` — forbid, not deny; nothing in this module will ever need the exception.
- **No new dependencies, including dev-dependencies.** Property-style tests use a
  hand-rolled deterministic PRNG (xorshift64\* is fine) in the test support code, seeds
  fixed and printed in assertion messages so failures reproduce. (This also keeps Miri
  fast; proptest-style shrinking is not worth a dependency here.)
- **New-code style:** `src/safe/` is new vocabulary, not transliterated C — idiomatic
  snake_case, real doc comments. Where a function mirrors C semantics, the doc comment
  names the C/ported counterpart (`/// Mirrors dec_golomb.rs dump_bits_aux /
  codec/decoder/core/src/bit_stream.cpp …`) — that's the diffability thread for later
  phases.
- **Detached-cursor principle (plan §2.1.3) is absolute:** no type in `src/safe/` stores
  a borrow. Cursors are positions; buffers are parameters. If you find yourself writing
  `struct Foo<'a>` for anything long-lived, stop — only the ephemeral view types
  (`PlaneCursorMut`, pool ref views) carry lifetimes, and they never outlive a call chain.
- **Anti-speculation rule:** every `pub` item needs (a) a differential test against
  existing behavior, or (b) a named consumer in plan §Phase 2/3/5/6 text. Nothing else
  gets built. It is explicitly fine for Phase 2/3 to come back and *add* methods; it is
  not fine for Phase 5 to discover a method nobody needs shaped wrong.
- **Deviation duty:** the plan's §2.2 sketches are contracts, not gospel. If
  implementation shows a better shape (it will, somewhere), build the better thing and
  update §2.2 in the same commit, with a line in the log. The plan must never lag the
  code.
- Commit discipline, style, revert-first-on-red, clean-session-exit: per
  `prompts/phase0.md` §3/§6, unchanged.

## 5. Tasks

Suggested split across the ~3 sessions this phase is sized at:
session A = T1–T3 (harness + planes + reader); session B = T4–T6 (writer + pool + grid);
session C = T7–T9 (error plumbing, Miri hardening, docs/bookkeeping). Take them in order;
any clean stopping point is fine if the exit protocol (§8) runs.

### T1 — Control run
Full battery (§3), results into the log. Everything must match the expected numbers
before you write a line. (If Phase 0 work happened first this session, this is its exit
gate re-used.)

### T2 — Module skeleton + test harness
1. `src/safe/mod.rs` (module docs: what this module is, the forbid policy, the
   detached-cursor principle, pointer to plan §2.2) + `pub mod safe;` in `lib.rs`.
   Submodules as they land: `plane`, `bits`, `pool`, `mb_grid`, `err`.
2. Test layout decision, implemented now:
   - **In-module unit tests** (`#[cfg(test)] mod tests` inside each `src/safe/*.rs`) —
     safe-only, forbid-compatible, Miri-fast. This is where invariants and edge cases
     live.
   - **Differential tests** in `tests/safe_vocab_differential.rs` (integration test —
     outside `src/`, so it may use `unsafe` to drive the existing kernels/readers/writers
     as reference implementations; every module in the crate is `pub`, so
     `openh264_rs::decoder::dec_golomb::…` paths are reachable).
   - The PRNG helper + tiny utilities go in `tests/common/` next to the existing safe
     sha1/y4m helpers (shared via the existing `#[path]` include pattern if needed).
3. Commit the skeleton once it builds green.

### T3 — `PaddedPlane` + plane cursors (`src/safe/plane.rs`)
The invariant being encoded (from `pic_queue.rs:177-330` / plan §2.2.1): a plane
allocation is `(pad_t + h + pad_b) × stride` bytes; the logical `(0,0)` origin sits at
byte `origin = pad*stride + pad`; logical coordinates are `isize` and negative into the
padding is legal by construction (decoder pads: 32 px luma, 16 px chroma — but **do not
bake those constants into the type**; `pad` and `stride` are constructor parameters,
because the C computes strides with its own alignment and Phase 5 must be able to match
it exactly).

Deliverables:
- `PaddedPlane` owning `Vec<u8>` + `{stride, origin, width, height, pad}`; constructors:
  `new(width, height, pad, stride)` (validates `stride >= width + 2*pad`, allocates
  zeroed) and `from_parts(buf, stride, origin, width, height)` (validates the invariant —
  this is the constructor Phase 2 shims will feed); accessors `at(x, y)`, `set(x, y, v)`,
  `row(y, x0, len) -> &[u8]`, `row_mut(…)`, `cursor(x, y)`, `cursor_mut(x, y)`,
  `as_slice()`/`as_mut_slice()` + `origin()`/`stride()` (the escape hatch Phase 2 kernels
  need for `chunks_exact` row-walking).
- `PlaneCursor<'a>` / `PlaneCursorMut<'a>`: `{buf, center, stride}` over a borrowed
  slice; `at(dx, dy)`, `set(dx, dy, v)`, `row(dy, dx0, len)`, `row_mut(…)`,
  `advance(dx, dy) -> Self`-style rebasing (by-value, cheap). Safe constructors from
  `(&[u8] / &mut [u8], center, stride)` — validated `center < buf.len()`; all deeper
  bounds enforcement is slice indexing itself. `#[inline]` on every accessor.
- Index math in one private `fn idx(center, dx, dy, stride) -> usize` doing checked
  `isize` arithmetic with an `as usize` cast that panics (via the slice index) when the
  result is out of range — a panic here is a port bug by definition (plan P13); document
  that.

Differential tests (the pointer-parity proof):
- Build a padded `Vec`, fill with PRNG bytes, construct both a `PaddedPlane::from_parts`
  view and a raw mid-pointer `p_data = buf.as_ptr().add(origin)` exactly as
  `AllocPicture` computes it. For a few thousand PRNG `(x, y)` pairs spanning the full
  legal range `x ∈ [-pad, width+pad)`, `y ∈ [-pad, height+pad)`: assert
  `plane.at(x,y) == unsafe { *p_data.offset(y*stride + x) }`. Same for `row` against
  `from_raw_parts`. Same through a cursor anchored at PRNG MB origins with PRNG
  `(dx, dy)` offsets — this is the exact access pattern of `decode_slice.rs:1944` and
  `svc_base_layer_md.rs:327-358`.
- Unit tests: constructor validation rejections, boundary coordinates (all four padding
  corners), `row` spanning negative-x into padding, out-of-range panics
  (`#[should_panic]`).

### T4 — `BsCursor` reader (`src/safe/bits.rs`)
Inventory first: `grep -n "pub.*fn" decoder/bit_stream.rs decoder/dec_golomb.rs` and
list every reader entry point with its exact semantics (known anchors:
`InitReadBits`, `DecInitBits`, `GetValue4Bytes`, the refill `dump_bits_aux` at
`dec_golomb.rs:128-150`, and the ue/se/bit readers built on it — take the real names
from the inventory, not from this prompt). Then implement `BsCursor` (plan §2.2.2):

- `#[derive(Clone, Copy, Default)] struct BsCursor { pos: usize, cur_bits: u32,
  left_bits: i32 }` — buffer is always a `&[u8]` parameter, never stored.
- Operations mirroring the inventory: init (both variants if their initial-fill
  semantics differ — check), `next_bits(buf, n)`, `read_one_bit`, `read_ue`, `read_se`,
  the peek/bit-count queries the decoder uses, each returning the **exact** error codes
  the existing code returns (`ERR_INFO_READ_OVERFLOW` etc. — reuse the constants from
  `decoder/decoder_context.rs`, don't redefine values).
- **The slop predicate is sacred:** the refill permits the cursor to sit 1 byte past the
  end (`iAllowedBytes + 1`) and reads 2 bytes at the cursor. Replicate the *predicate*
  exactly; express the reads through `buf.get(..)` so they are in-bounds-or-error rather
  than in-bounds-by-guard-bytes — **but** pin the observable behavior (consumed
  positions, returned bits, error codes) to the old implementation via differential
  tests on truncated buffers of every length 0..=8 around the boundary. The
  guard-byte decision for the real decoder buffer is Phase 3's (plan P6); your job is an
  API whose behavior on short input is *identical from the caller's view*. If exact
  equivalence at the 1-past-end read genuinely requires seeing bytes beyond `len`
  (because the old code reads allocation slack), that is a **finding** — record it in
  the findings doc with the failing case, make `BsCursor` return the error code path,
  and flag it for the Phase 3 wiring decision. Do not silently pick either semantics.
- Differential tests: PRNG buffers (lengths 0..~64 plus a few KB), drive old
  (`SBitStringAux` + inventory fns, via unsafe in the integration test) and new
  (`BsCursor`) through identical PRNG op sequences (mixed `next_bits(1..=24)`, `ue`,
  `se`, aligned/unaligned starts); assert bit-for-bit equal outputs, equal error codes,
  equal final cursor byte-position. Include the emulation-prevention-free RBSP case only
  — EBSP handling is `nalu.rs`'s job, not the cursor's (note that in the docs).

### T5 — `BsWriter` (`src/safe/bits.rs`)
Reference = `encoder/vlc_encoder.rs` (F2: the canonical copy). Inventory its surface
(`BsWriteBits` :367, `BsWriteOneBit`, `BsWriteUE`, `BsWriteSE`, `BsGetBitsPos`,
`WRITE_BE_32`, plus the init/flush functions — take exact names/semantics from the
file). Implement:

- `#[derive(Clone, Copy, Default)] struct BsWriter { pos: usize, cur_bits: u32,
  left_bits: i32 }`; ops take `&mut [u8]`; 32-bit big-endian accumulator flush exactly
  as `WRITE_BE_32` + `pos += 4`; `bits_pos()` mirroring `BsGetBitsPos`.
- **Canonical semantics, checked bounds:** the canonical writer has no end check —
  buffer sizing is the caller's contract (plan §2.2.2 item 2). The safe writer keeps
  canonical *output* semantics; slice indexing supplies real bounds, and an
  out-of-space write is a panic (= a pre-existing sizing bug made loud, not new
  behavior on any in-contract path). Doc-comment the F2 situation: three other copies
  exist with divergent guards; Phase 3.2 decides their fate; `BsWriter` deliberately
  matches only the canonical.
- **`Copy` snapshot/rollback is a feature, test it:** `let saved = writer;` … restore —
  this is the safe replacement for `pBsStackBufPtr` stash/pop
  (`svc_set_mb_syn_cavlc.rs:1057-1076`); a differential test replays a
  write–snapshot–write–rollback–write sequence against the old code doing the same via
  cursor save/restore, asserting identical final bytes.
- Differential tests: PRNG op sequences (`write_bits(1..=32)` values masked in-contract,
  `ue`, `se`, one-bit; hit the `iLen == iLeftBits` boundary deliberately and often)
  against the canonical fns on identical pre-sized buffers; assert byte-identical
  output, identical `bits_pos()`. Round-trip tests: `BsWriter`-written PRNG streams read
  back by `BsCursor` *and* by the old reader.

### T6 — `Pool` + handles (`src/safe/pool.rs`) and `MbGrid` skeleton (`src/safe/mb_grid.rs`)
Pool (plan §2.2.3, generalized — record the generalization in §2.2.3 when you commit):
- `Pool<T> { slots: Vec<T> }` with a typed-handle pattern: `Id` newtype over a small
  index (`u8`-sized reasoning, but store `u32`/`usize` internally; `PicId` becomes a
  type alias or newtype over this in Phase 5). Handle identity == equality; `Copy`,
  `Eq`, `Debug`.
- `get`, `get_mut`, `pair_mut(a, b) -> (&mut T, &mut T)` (panics on `a == b`), and the
  split-borrow workhorse `mut_and_refs(cur, refs: &[Id]) -> (&mut T, PoolRefs<'_, T>)`
  — `cur ∈ refs` panics (port bug by definition; document). Implement with safe
  mechanics only: `slice::get_disjoint_mut` for the pair; for `mut_and_refs`, the
  iterate-`iter_mut`-and-select pattern (each element yielded once, collect the `&mut`s
  you need, downgrade the ref set to `&T`) — no unsafe, no `RefCell`.
- **D1 (plan §10) — implement the recommendation, note it as now-decided-by-default:**
  plain indices with release semantics identical to C recycling; a
  `#[cfg(debug_assertions)]` generation counter per slot, bumped on
  `replace`/`recycle`-style operations, checked in `get*` — debug builds catch stale
  handles, release behaves exactly like the C++. Unit tests for both behaviors
  (`#[cfg(debug_assertions)]`-gated for the panic case).
- Unit tests only (nothing to differ against): aliasing rejections, disjointness,
  generation checks, iteration.

MbGrid skeleton — **geometry only**, the field set belongs to Phase 5/6:
- `MbDims { mb_width, mb_height }` with `mb_xy(x, y)`, `xy_of(mb_xy)`, `count()`, and
  grid-bounds neighbor calculus: `left/top/top_left/top_right(mb_xy) -> Option<usize>`
  mirroring exactly the C index arithmetic (`xy-1` guarded by `x>0`, `xy-mb_width`
  guarded by `y>0`, etc.). Slice-idc availability is Phase 5 *logic* layered on top —
  keep it out.
- `MbArray<T> { data: Vec<T>, dims }` indexed by mb_xy with the neighbor accessors
  riding on `MbDims`.
- Unit tests: edge/corner MBs, single-row/single-column grids, exhaustive
  neighbor-vs-hand-computed comparison on a PRNG-sized grid.

### T7 — Error plumbing (`src/safe/err.rs`)
Minimal, per D6 (i32 codes stay internal until Phase 8/9): a transparent
`ErrInfo(pub i32)` (or equivalent) used by `BsCursor` return types, `const`-mapped to
the existing `decoder_context.rs` codes (reuse, don't redefine), `From`/`into_i32`
helpers. No error hierarchy, no `thiserror`-alike, no display formatting beyond `Debug`.
If T4 ended up returning bare i32s and it reads fine, this task may shrink to
documentation — prefer the smallest thing that keeps Phase 3 call sites honest.

### T8 — Miri protocol
- `cargo +nightly miri test --lib` — the in-module safe tests: **must be clean**, no
  exceptions.
- `cargo +nightly miri test --test safe_vocab_differential` — this executes the *old
  unsafe code* under Miri too. Two possible outcomes per test:
  clean (great — free UB coverage of the reference implementations), or Miri flags the
  **old** side (plausible: the reader's 1-past-end read, punning in reference kernels).
  A Miri hit on old code is a **finding** (append a Phase 1 section to
  `phase0_findings.md` or start `phase1_findings.md` — follow the existing doc's
  format): record it, mark that specific test `#[cfg_attr(miri, ignore)]` with a comment
  naming the finding, and keep it running under plain `cargo test`. The safe side must
  never need the ignore.
- Record the exact Miri invocations + results in the log; add them to `gates.sh` as a
  phase-1+ gate (fast subset: `--lib` always; the differential file under Miri weekly or
  at phase exits if slow).

### T9 — Bookkeeping
Per `prompts/phase0.md` §6 conventions:
1. Plan: add the "### Phase 1" checklist to `## Progress` (mirror the Phase 0 style,
   with commit hashes); update §2.2 for any contract deviations you made; check the D1
   note in §10.
2. Log entry: control vs final gates, what landed, findings, next session's first action
   (if Phase 1 completes: "Phase 2, pilot `decoder/decode_mb_aux.rs` per plan §Phase 2 —
   the smallest kernel family — to validate the plane API before mass conversion").
3. Auto-memory: extend the safety-refactor workstream note (Phase 1 state, where the
   vocabulary types live, the Miri gate).
4. Clean tree, full battery green, ratchet `check` green (new module adds zero unsafe —
   any ratchet increase is a bug in your work).

## 6. Gates for this phase

- **Per-commit:** `cargo test` + `cargo test --release` (294+new passed / 0 failed /
  20 ignored — the *existing* 294 and the ignore set are invariant; only your new tests
  may add).
- **Checkpoint (each T-task lands) and session end:** per-commit gates + ratchet `check`
  (if T6 landed) + `cargo +nightly miri test --lib`.
- **Session end additionally:** `bash sweep.sh st mt def` both profiles (F3 retry rule
  for `t=4 sm=3` release only) + differential file under Miri (T8 protocol) + bench
  smoke (`decode_1080p_bench`; `c_vs_rust_bench` only with `FFMPEG` set — else skip it
  loudly, per `perf_baseline.md`).
- Phase 1 changes no codec behavior: **every pre-existing number is frozen.** Any drift
  in the 294, the 341/341, a hash, or the ignore-20 means stop, revert, investigate —
  in that order.

## 7. Explicit non-goals for Phase 1

No wiring into codec code (no shims yet — Phase 2/3 create shims *in the old modules*
at adoption time). No kernel ports (not even one "just to try the API" — Phase 2's
pilot does that deliberately, gated). No CABAC engine, no NAL/`RawDataBuffer`, no
EBSP/emulation-prevention handling, no `ExpandPicture` methods, no `Picture` struct, no
allocator work, no threading types. No changes to `Cargo.toml` (no deps, no features,
no crate-type — cdylib is Phase 8). No edits under `codec/`. No renames or cleanups in
existing modules. No fixing F2/F3 or any fuzz finding. And no starting Phase 2 early if
the session has time left — spend surplus time on more differential coverage and better
docs for the types you built; Phase 5 will collect the interest.
