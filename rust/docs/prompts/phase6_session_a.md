# Phase 6, session A — the encoder Miri probe, and the Miri budget

One subject: **the encoder gets aliasing coverage, and the gate stays affordable.**
No conversions, no `c_void` deletion, no F52 adjudication, no `SPicture` work —
those move to session B and later (`phase6.md` §6 is updated accordingly).

Why this is a session of its own: the encoder has **zero encode-path Miri
coverage** today. The only encoder-adjacent Miri test is
`encoder_deblocking_table_installs_the_common_shims`
(`tests/kernels_differential_phase2.rs:2254`), which is `#[cfg_attr(miri, ignore)]`
and installs a table rather than encoding. **F47 is the precedent**: real UB on the
ordinary CAVLC path survived five phases because no probe drove it, and the probe
that finally did found it on its first execution. Converting the encoder before it
has a probe repeats that exactly.

Governing: [`phase6.md`](phase6.md) §1–§3. **S15** (no Miri skip without a
finding), **S32** (the Miri cost model), **S33** (a recorded outcome is a
measurement, never a prediction), **S24** at every face. Counts at `5f2bd711`;
re-grep at each face's open.

## The finish rule

**This session has no hand-off.** It ends when §5's done-test reads met, or at a
blocker only Eugene or the steward can clear, named. A size is never a stop
(**S31**: state lives on disk — compact, re-read this file, continue). A clean
face boundary is a checkpoint, not an exit. A question takes the decision ladder
(settle by reading / lint-scope defaults to the enumerated exception / behaviour
questions park with a record and the session continues).

**And the one specific to this session: the probe finding UB is the probe
working.** Every decoder probe found something on its first run — F23/F24, F35,
F47. A red probe is the session succeeding, not a reason to stop: record the
finding, fix it if it is Phase 6's, park it with its owner if it is not, continue.

## 0. Start

1. Commit the inherited doc tail (phase6.md §5/§6, this brief).
2. Open per **S27** (Phase 5b session D closed `exit` `OVERALL: PASS` 13/0/1):
   build both profiles + `--all-targets`, tests, ratchet, census.
3. S33 on every number written.

## 1. Face 0 — the Miri baseline, and the probe-reach audit

S32's numbers are from an older tree. Re-measure before adding anything:

1. Run the `exit`-level Miri steps and time **each step separately** — the `--lib`
   run and each `--test <differential>` run (`gates.sh` lines ~339–376:
   `run_miri lib … -Zmiri-ignore-leaks --lib -- "${MIRI_SKIPS[@]}"`, then one
   `run_miri` per differential target). Record wall-clock per step.
2. Time the three decoder probes **individually** (`cargo +nightly miri test --lib
   -- <probe name>`), so the marginal cost of one probe is a number rather than
   S32's inference. Last recorded: 3/3 in 1099.7s together.
3. Record how much of each run is **compile** vs **interpret** (a second identical
   invocation is warm — the delta is the compile share).

### Decoder probe reach — retire what is redundant

The three probes cost ~1100s and were built to find UB in raw decoder code that
no longer exists: `src/decoder/` has **0 `unsafe fn` and 6 `unsafe {` blocks**,
all inside three FFI items — `api_alias`/`api_alias_mut`
(`decoder_context.rs:1230`, `:1243`) and `data_ptr` (`picture.rs:725`). All three
drive the same vtable driver and all three produce frames, so they may reach an
identical set. Measure it, then retire the redundant ones:

4. Instrument the six blocks and the `api/` driver's raw paths (counter,
   `eprintln!`, or coverage tooling — record which method).
5. Run each probe alone; record the set of blocks each executes.
6. **Sets identical** → keep only the widest-reaching probe (expected: the grid
   probe, `grid_48x32` — CABAC, the 8×8 transform, B slices, neighbours); take
   the other two off the Miri target.
   **Any probe reaching a block no other reaches** → it stays; name the block.
7. Retire by Miri exclusion only: `#[cfg_attr(miri, ignore)]` at the test, or a
   named filter in `MIRI_SKIPS`, reason at the line. The tests keep running in
   the normal suite — show the suite count unchanged.
8. Record per **S15**: the justification plus the owner — *Phase 8 re-instates
   decoder probe coverage when it rewires the boundary* (`api/` is 240 raw
   pointers and 48 `unsafe fn`, exempt until then). Add that line to the Phase 8
   inheritance list.

Constraints: retirement is decided by **measured reach**, never by runtime and
never by "the decoder is safe now". **At least one decoder probe stays live** —
Phase 6 edits `common/` (`copy_mb`, `expand_pic`, `mc`), which the decoder calls.

Write the timings table and the reach sets into the log; they are the control for
faces 2 and 3.

## 2. Face 1 — the probe

**Follow `drive_decoder_over`'s pattern exactly** — it is in
`src/api/codec_api.rs`, `mod abi_test_driver` (moved there at T5b.6 so it drives
the real vtable), signature
`pub(crate) fn drive_decoder_over(stream: &[u8]) -> (usize, Option<(i32,i32)>, i32)`,
and its own doc comment records why: an earlier draft used
`(*p_decoder).Initialize(&param)` and **Miri rejected it before decoding began**,
because `ISVCDecoder::Initialize` takes `&mut self` over the 8-byte vtable base
while the thunk writes the impl object at offset 0x20. That is **F23**, it is
Phase 8's, and the encoder's `ISVCEncoder` has the same shape.

1. Write `drive_encoder_over(...)` beside it: **raw-pointer-based calls, not
   `&mut self` convenience methods.** If the encoder twin of F23 exists, the probe
   must route around it the way the decoder's driver does — **do not fix F23 here**
   (Phase 8's); record the encoder twin's existence in F23's entry if it is not
   already named there.
2. One probe test, `#[test]`, in the library so the `--lib` Miri step picks it up.
3. **Placement trap, and it is a real one**: the `--lib` step runs with
   `MIRI_SKIPS=(--skip wels_thread_pool --skip encoder_ext)`, and `--skip` matches
   on **test-name substring**. A probe placed in `encoder_ext.rs` — or named
   anything containing `encoder_ext` — is silently skipped, and `gates.sh`'s
   "ran 0 tests" guard will not catch it because other tests still run. That is
   the F17/F18 class (a gate reporting pass while running nothing). **Name and
   place the probe so neither filter matches it, and prove that by running the
   `--lib` step and seeing the probe's name in the output.**

**Non-vacuity, asserted in the test body** — the decoder probes' pattern
(`decode_slice.rs:5678–5768`), where each assertion carries the reason it exists:

- Output bitstream length > 0, and frame count as expected — the encode loop ran.
- **At least two frames, the second inter-coded** — otherwise no ME/MD path
  executes at all.
- **Motion between the frames.** Two identical input frames encode to all-skip
  macroblocks and motion estimation does essentially nothing; synthesize a shifted
  pattern so the search runs. Assert something that can only be true if it did.
- **At least a 3×2 macroblock grid (48×32).** This is **F34's lesson**, and the
  decoder's grid probe carries it as a live assertion with the comment *"a stream
  without neighbours covers nothing this test exists for"*. A single-macroblock
  encode has no neighbour, so no neighbour-dependent MD/ME path runs.

**Prove coverage, do not assert it** (the F21 rule): revert one known-live line on
the path the probe claims to cover, show the probe goes red, restore. Record the
line and the observed failure in the log — **measured, not predicted** (S33).

## 3. Face 2 — does S32 transfer to the encoder?

**S32 was measured on the decoder**: cost is instantiation, flat against decoded
work (36 macroblocks 268.9s vs 16 macroblocks 196.0s), so shortening a stream saves
nothing and only probe *count* is the knob. **The encoder may not obey it** —
motion estimation and mode decision are per-macroblock search, not per-macroblock
copy, so per-frame work may dominate init.

Settle it with three timed runs of the probe, one variable at a time:

1. baseline (the face-1 configuration),
2. **frame count** halved or doubled,
3. **frame size** at one step smaller and one step larger (keeping ≥ 3×2 MBs).

If the readings are flat, S32 transfers and the probe's config is free — say so.
If they scale with work, **S32 gains an encoder clause** and the probe is sized to
the cheapest configuration that still passes face 1's non-vacuity assertions. Write
the amendment into plan §7.6 either way; a null result is a result (S33).

## 4. Face 3 — the placement decision, made with the numbers

Total Miri time is now measurable. Decide and **write down**:

1. **Which gate level runs the encoder probe** — `full`/`exit` alongside the
   decoder's three, or `exit` only. Phase 6 wants a probe per seam, so prefer the
   cheaper placement only if face 2 says the probe is expensive and irreducible.
2. **The budget statement**: the exit battery's Miri share, before and after, in
   seconds — where "after" includes both the encoder probe added **and** whatever
   face 0's reach measurement retired. `gates.sh exit` minus Miri was 103s at
   S32's measurement. The honest target is that the encoder gains coverage while
   the total does **not** grow: one live probe per codec, each earning its seconds.
3. **Levers, if the total is unacceptable** — in this order, and each with its
   number: shrink the configuration (only if face 2 says work-dominated); move the
   probe to `exit` only; scale loop counts under `cfg!(miri)` (the existing
   mechanism — `tests/kernels_differential_phase2.rs:38`,
   `tests/safe_plane_differential.rs:25`, both `if cfg!(miri) { (n/100).max(2) }`);
   accept the cost and say so.

**Forbidden, because they buy time by buying blindness**:
`-Zmiri-disable-stacked-borrows`, `-Zmiri-disable-validation`, and any other flag
that weakens aliasing or validity checking — this gate exists to find UB, and a
faster gate that cannot find it is worth nothing. `-Zmiri-ignore-leaks` stays as
it is (already on for `--lib`, and deliberate: the port allocates C-style on
purpose).

**Retiring a decoder probe from the Miri target is permitted only on face 0's
measured reach** — never on runtime, never on "the decoder is safe now" as an
argument. The probe stays a test; what is retired is its Miri execution; the
record names Phase 8 as the owner that re-instates it. A probe that reaches an
unsafe block no other probe reaches is not retired at any price.

## 5. Done-test, and the close

1. `drive_encoder_over` exists beside `drive_decoder_over`, raw-pointer-based, and
   the encoder probe is green — **or red with its finding recorded and owned**.
2. The probe's coverage is **proved** by an observed red-under-revert, with the
   reverted line named.
3. The probe is **verified to actually run** in the `--lib` Miri step — its name
   appears in that step's output, neither skip filter matches it.
4. **Each decoder probe's reach is measured** and each is either kept on the Miri
   target with the block it uniquely reaches named, or retired from it with the
   record and the Phase 8 re-instatement pointer written. The retired ones still
   run as ordinary tests — show the suite count unchanged.
5. Face 0's baseline table and face 2's three readings are in the log, and S32
   either transfers (stated) or has its encoder clause written into plan §7.6.
6. The placement decision and the budget statement are written, with the exit
   battery's Miri share before and after.

Close: log entry (≤ 30 lines), plan §0's gate row updated with the new Miri
totals, `phase6.md` §6 updated to show session A spent and session B next. Full
battery at `exit` level; F3 per S14 — and note this phase's clause: every encoder
change moves the binary, so step 0's hash shortcut will essentially never apply.
No session span: this session adds a test and changes no production code
(D-perf-6, and S2b — a span over no hot-path change measures nothing).

## 6. Non-goals

No conversions of any kind — not one signature. No `c_void` deletion, no F52
adjudication, no `SPicture` ids (session B and later). No decoder work. No F23
fix (Phase 8's — route around it). No threading work (Phase 7's). No second probe
unless face 1's cannot reach CABAC **and** a later face is named that needs it —
S32 says probe count is the budget, so a second probe is a decision with a number
behind it, not a convenience.
