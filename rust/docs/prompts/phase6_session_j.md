# Phase 6, session J — the deny sweep under D-exit-1, and the phase closes

You are executing the final session of Phase 6. Work through the steps in order.
Commit per unit of work. Run the gates exactly as stated. Report at the end in
the format given in the last section.

## Context

- Repo: `/Users/eugene/projects/openh264`, branch `rust3`; crate
  `rust/crates/openh264-rs/`, paths below relative to its `src/`. The C++ tree
  at `codec/` is the behavioral reference; byte parity is gate-enforced.
- Session I left the phase open on a measured finding (**F65**,
  `rust/docs/phase6_findings.md:490`): the deny sweep names **894+ items**, at
  most 117 of which this phase may convert — the rest are raw-pointer
  signatures whose *parameter families* belong to Phases 7 and 9 by this
  phase's own do-not-touch table. §7's condition 2 named only four lawful
  allow categories, and several hundred survivors are none of them.
- **The decision is made — D-exit-1 (steward, 2026-08-20, plan §7.4): F65's
  option 1.** A fifth lawful category exists: **`port-raw(Phase 7)` /
  `port-raw(Phase 9)`** — a port-internal raw-pointer signature whose
  retirement is named to the phase that owns its family. Every allow item
  carries exactly one of five tags; an untagged or unowned `unsafe` is a build
  error; a `port-raw` tag dies with its item when the owning phase converts
  it. Condition 1 (deny on every non-MT module) stands unchanged. Your job is
  to execute the sweep under that ruling and close the phase.
- State at your start (commit `b41e481e`, tree clean): `pPSOVector` deleted;
  `pFuncList` is `Box<SWelsFuncPtrList>` (allocator hits: 4 Phase-7 + 8
  dormant, nothing else); `*mut SWelsFuncPtrList` 57 → 28 (22 survivor
  functions, category `cursor`, owner Phase 9 — five fn-pointer slot types
  name the raw table in their own signatures); `*mut sWelsEncCtx` at 298
  lines; **step 2 of session I's brief was not begun** — that is your step 1.
- Documents you will update at the close:
  `rust/docs/safety_refactor_log.md` (session entry + the phase-close entry),
  `rust/docs/safety_refactor_plan.md` (§0 status row incl. the **Phase 6
  COMPLETE** row), `rust/docs/prompts/phase6.md` (§6 rows I and J),
  `rust/docs/perf_baseline.md`, `rust/docs/phase0_findings.md` (only on an F3
  measurement).
- Commits: `refactor(T6.J<n>): …` / `gate(T6.J<n>): …` / `docs(T6.J-close): …`.

## Hard rules

1. **Gates.** `bash rust/tools/gates.sh commit` in every commit;
   `gates.sh family` at each step's end (sweeps: **369/369 both profiles**);
   the close runs the full battery per step 5.
2. **Miri.** Only via the gates, as `MIRI_SCOPE=encoder …` (252 tests ≈600 s)
   — **except step 5's final battery, which runs unscoped** (349 tests
   ≈1411 s): the one unscoped run decision D-gate-2 reserves for the phase
   exit. All seven probes (4 encoder + 3 decoder) must be green by name in its
   output.
3. **Perf.** One measurement at the close: `rust/tools/perfpair.py build` at
   `b41e481e` and at HEAD, `run … --pairs 7`, `null … --pairs 7`. On a median
   breach of the null band: bisect the session's commits with perfpair
   **before** changing anything; fix or record the measured contributor with
   its owner. Restate at the close: cumulative encoder deficit ≈ **+15…+17%**,
   tripwire +25% median (D-perf-4), D-perf-6's recovery is Phase 9's.
4. **Counts are anchors** (measured 2026-08-20 at `b41e481e`). Re-run the
   given grep first; trust the grep over the brief and say so in the log.
5. **Pins.** Any layout change re-measures its `assert_size!` /
   `assert_ctx_offset!` in `encoder/abi_guard.rs`, same commit, both profiles
   where split. Step 1 changes no layouts; expect no pin churn.
6. **The `&mut`-from-raw rule** (step 1). `&mut *pCtx` at a call site whose
   caller keeps using its own `*mut sWelsEncCtx` afterwards is UB that
   compiles silently:

   ```rust
   // BAD — the &mut pops pCtx; the read after the call is UB under Miri:
   callee(&mut *pCtx);
   let x = (*pCtx).iFrameNum;
   // GOOD — convert top-down: the api boundary derives &mut from the
   // owning Box once per call and passes it down; a callee that still
   // needs raw receives the raw it was given, never a fresh &mut.
   ```

   Convert from the call-tree root down, whole closures per commit, never
   bottom-up.
7. **Sweep flake (F3).** One failing configuration with the signature — `mt`
   preset, `sm=3`, `t=2` or `t=4`, and the Rust output has **any wrong
   length** (empty, short, or long: the fingerprint is the length mismatch,
   not its direction) → re-run that exact configuration 5×; all byte-identical
   → append the measurement to `phase0_findings.md` F3 and continue. Anything
   else: stop and fix; it is yours.
8. **No behavior change.** No encoded byte moves.
9. **Overflow.** If a step cannot finish, stop at a whole-closure boundary and
   report; do not weaken a §7 condition, and do not run step 5's close on a
   partial sweep.

## The five-way tag — assignment rules for step 2

Tag format, on the line above each `#[allow(unsafe_code)]` item (grep-able):

```rust
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
```

Assignment, driven by what the item's signature names (the residue census in
F65 is the map):

| the item | tag |
|---|---|
| `#[no_mangle]` exports (5) and anything crossing the C ABI | `C-ABI` (Phase 8) |
| accessors/writers over owned storage that hand out cursors and carry their derive-twice Miri tests — the frame-bitstream writers (19 enumerated in H's log), pool accessors, the named neighbour-walkers, the 22 dispatch-table survivors | `cursor` (owner noted in parens where it is Phase 9's de-virtualization) |
| MT seams in non-MT files (the two MT files themselves get **no deny**) | `MT` (Phase 7) |
| anything already tagged `SCREEN_CONTENT(dormant: Phase 10)` — the census is **24** tagged items | `SCREEN_CONTENT(dormant)` |
| signatures naming `*mut/*const SSlice` (105) or the MT-adjacent `SDqLayer` share | `port-raw(Phase 7)` |
| signatures naming `*mut/*const u8` plane/byte cursors (177), `SMbCache` (66), `SMB` (45), the Phase-9 `SDqLayer` share, `i32`/`i16` coefficient cursors (77/51), the **four** `pMvdCost*` record fields | `port-raw(Phase 9)` |

An item that fits two tags takes the more specific one (`cursor` beats
`port-raw` when the tests exist; `C-ABI` beats everything). Zero items may end
untagged — that is condition 2's test.

## Step 1 — the context parameters: the 117, root-down

Of 272 `unsafe fn` naming `*mut sWelsEncCtx`, **117 have it as their only raw
parameter type** (session I's measured split) — those convert to safe `fn`
taking `&mut sWelsEncCtx` (or `&` where read-only). The other 155 keep their
raw context parameter (converting it beside another raw parameter derived from
the same context is the aliasing trap) and will be tagged in step 2.

Procedure: start at the api entries (`WelsInitEncoderExt`,
`WelsEncoderEncodeExt`; `wels_encoder_ext.rs` keeps its raw handle — the
reference is born at that boundary from the owning `Box`, once per call).
Descend closure-by-closure; one reachability closure per commit. Per function,
two questions: (Q1) does it hold a context-derived raw cursor across a call
that reaches the context again? (Q2) does it reach one object through two
parameters? Two nos → convert. Any yes → survivor list entry:
`fn name — Q1/Q2: <one line>`, with its step-2 tag.

Do not touch `wels_task_management.rs` (12 lines) or `wels_encoder_ext.rs`
(8 lines) — pre-attributed.

Accept: `grep -rn '\*mut sWelsEncCtx' --include='*.rs' src/encoder
src/processing | wc -l` reads ≈ 298 − (the converted lines), and every
remaining line is on the survivor list or in the two untouched files;
`gates.sh family` green; `MIRI_SCOPE=encoder` Miri green.

## Step 2 — the deny sweep, tagged five ways

1. Fix the **17 files** where session I's experiment found
   `#![deny(unsafe_code)]` failing to parse: an inner attribute must sit at
   the very top of the file, after the `//!` module docs and before any item
   or import — `decoder/decoder_core.rs:43` is the model.
2. Add `#![deny(unsafe_code)]` to all **36** non-MT modules of `src/encoder` +
   `src/processing`. The two MT files instead get the module-top comment:
   `// deny(unsafe_code) lands with Phase 7; this file is the thread machinery.`
3. Drive the tagging from the compiler: build, and for every diagnostic add
   `// unsafe-cat: <tag>` + `#[allow(unsafe_code)]` per the assignment table.
   Script-assist is fine; the *category choice* per item is yours, not the
   script's, for anything the table does not decide mechanically.
4. Record the table: per file × per category, item counts. Re-baseline the
   ratchet (`bash rust/tools/unsafe_ratchet.sh`) and explain the deltas.

Accept: `grep -rln 'deny(unsafe_code)' src/encoder src/processing | wc -l` =
36; `grep -rn 'allow(unsafe_code)' src/encoder src/processing | wc -l` equals
the tag count `grep -rn 'unsafe-cat:' … | wc -l` (no untagged allows); the
build is clean; `gates.sh family` green.

## Step 3 — condition 3: the residue table

Produce the `raw_ptr` residue table for the close log: per raw-pointee family,
code sites vs prose (comments), category, owner — the ratchet plus the family
greps you have been using. This is enumeration only; no code moves.

## Step 4 — §7's checklist

Write the five conditions into the log entry, each with its one-line evidence:
(1) the deny grep = 36; (2) allows = tags, zero untagged, counts per category;
(3) the step-3 table; (4) step 5's battery + the perf restatement; (5) the
handoffs — already written at plan §5 by session I; verify the four blocks
still match the tree and reference them.

## Step 5 — the close

1. The span (hard rule 3), into `perf_baseline.md`.
2. `bash rust/tools/gates.sh exit` **without** `MIRI_SCOPE` — the full
   unscoped battery. All seven probes green by name. Adjudicate any F3 hit
   per hard rule 7.
3. The phase-close log entry — Phase 6's arc: encoder-side `raw_ptr` 2669 →
   final, `unsafe_fn` 710 → final, allow items per category (the step-2
   table), findings F57–F65 each with disposition, sweep growth 341 → 369,
   the nine session spans, cumulative perf against D-perf-4 and D-perf-6.
4. Plan §0: the **Phase 6 COMPLETE** row (model: the "Phase 5b COMPLETE" row
   — what closed, the numbers, what each later phase inherits, with D-exit-1
   cited for the fifth category), sessions I and J marked spent, **Phase 7
   named next**. phase6.md §6: rows I and J to SPENT. Commit
   `docs(T6.J-close): …`.

## Do not touch

| what | owner |
|---|---|
| `slice_multi_threading.rs`, `wels_task_management.rs`, MT context members, `sSliceBs.pBs`, `DynamicSliceBs`, `pMemAlign`, `common/memory_align.rs`, `common/wels_thread_pool.rs` | Phase 7 |
| `wels_encoder_ext.rs` internals | Phase 8 |
| SAD/SATD kernels, `SMbCache`'s 72 kernel sites, `*mut SMB`'s named survivors, per-MB plane cursors, the four `pMvdCost*` record fields, the {LTR, rc, ref-list} accessor cost, the 22 dispatch survivors' de-virtualization | Phase 9 |
| anything tagged `SCREEN_CONTENT(dormant: Phase 10)` (24 items) | Phase 10 |

## Report back, in this order

1. One line: phase closed or not; the unscoped `exit` verdict; HEAD; tree
   state.
2. The five §7 conditions, each with its evidence one-liner.
3. Step 1: converted count (target 117) and the survivor split by tag.
4. Step 2: modules denied (target 36), allow items per category, untagged
   count (target 0), the 17 parse fixes confirmed.
5. The span and the cumulative restatement.
6. Anything found and not fixed, with owner.
7. If the phase did not close: what blocked, at which step, and the remainder.
