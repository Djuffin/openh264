# Phase 6, session I — the deny sweep and the phase close

You are executing one session of a long-running refactor. Work through the steps
in order. Commit per unit of work. Run the gates exactly as stated. Report at the
end in the format given in the last section.

## Context

- Repo: `/Users/eugene/projects/openh264`, branch `rust3`. The Rust crate is
  `rust/crates/openh264-rs/`; all source paths below are relative to its `src/`.
  The C++ tree at `codec/` is the behavioral reference — byte parity against it
  is enforced by the gates; its code style is not a constraint.
- The project: a full Rust port of the openh264 encoder+decoder, byte-identical
  to the C++, being converted from raw-pointer C-style Rust to safe Rust. The
  decoder is done (`src/decoder/` carries `#![deny(unsafe_code)]` on all 22
  modules, 3 allow items). Phase 6 is the encoder's structural rewrite; this is
  its **final planned session**.
- State after session H (start commit `5be7b175`, tree clean): the encoder
  context `sWelsEncCtx` (`encoder/encoder_context.rs:469`) has a real
  constructor, all its members but one are owned (`Vec`/`Box`) or ids, the
  custom allocator is down to 15 call sites, and every remaining raw-pointer
  family has been measured and assigned an owner phase. What is left for this
  session: one dead field, one unowned member (`pFuncList`), the context
  *parameter* spellings (`*mut sWelsEncCtx` → references), the
  `#![deny(unsafe_code)]` sweep, and the phase-close bookkeeping.
- Documents you will update at the close:
  `rust/docs/safety_refactor_log.md` (append the session entry),
  `rust/docs/safety_refactor_plan.md` (§0 status row, §4 phase rows),
  `rust/docs/prompts/phase6.md` (§6 session row),
  `rust/docs/perf_baseline.md` (the span),
  `rust/docs/phase0_findings.md` (only if an F3 measurement occurs).
- Commit messages: `refactor(T6.I<n>): <what changed>` /
  `gate(T6.I<n>): …` / `docs(T6.I-close): …`, with a body saying what and why.

## Hard rules

1. **Gates.** `bash rust/tools/gates.sh commit` must pass in every commit.
   `bash rust/tools/gates.sh family` at the end of every step (it runs the
   differential sweeps: expect **369/369 in both profiles**). One full
   `bash rust/tools/gates.sh exit` at the close — see step 5 for its Miri mode.
2. **Miri.** Run Miri only via the gates, only at step ends where stated, always
   as `MIRI_SCOPE=encoder bash rust/tools/gates.sh …` (252 tests, ≈600 s) —
   **except the final close battery in step 5, which runs unscoped** (349
   tests, ≈1411 s; this is the phase's handoff gate and the one unscoped run
   allowed by decision D-gate-2).
3. **Perf.** Exactly one stream measurement for the whole session, at the close:
   `rust/tools/perfpair.py build` at the start commit and at HEAD,
   `perfpair.py run <start> <head> --pairs 7`, plus `perfpair.py null <head>
   --pairs 7` for the floor. If the median breaches the null band: bisect the
   session's commits with `perfpair.py` pairs per commit **before** changing
   anything, then fix the measured contributor or record it with a Phase-9
   owner. Do not revert on suspicion. Cumulative encoder deficit stands at
   ≈+15…17%; the tripwire is +25% median — restate both numbers in the close.
4. **Counts are anchors.** Every number and line anchor below was measured
   2026-08-20 at commit `250b3732`. Re-run the given grep before acting on it;
   if it disagrees, trust the grep and say so in the log.
5. **Layout pins.** Any struct whose fields change gets its `assert_size!` /
   `assert_ctx_offset!` in `encoder/abi_guard.rs` re-measured and re-pinned in
   the same commit (both profiles where the numbers split). Never delete an
   assertion.
6. **The `&mut`-from-raw rule** (governs step 2). Creating `&mut *pCtx` while
   the caller keeps using its own `*mut sWelsEncCtx` afterwards is UB that
   compiles silently:

   ```rust
   // BAD: callee(&mut *pCtx) pops pCtx; the (*pCtx) read after is UB.
   callee(&mut *pCtx);
   let x = (*pCtx).iFrameNum;
   // GOOD: convert top-down. The api boundary owns the Box; a &mut is
   // derived from the owner once per call and passed down; callees that
   // still need raw get the raw they were given, never a fresh &mut.
   ```

   Therefore step 2 converts **from the call-tree root down**, never bottom-up.
7. **Sweep flake (known issue F3).** If exactly one sweep configuration fails
   with: `mt` preset, `sm=3`, `t=2` or `t=4`, and the Rust output has the
   **wrong length** (empty or short — a short stream is a known shape since
   measurement 85): re-run that exact configuration 5 times. If all 5 are
   byte-identical, append a measurement entry to
   `rust/docs/phase0_findings.md`'s F3 section and continue. Any other failure:
   stop and fix it; it is this session's.
8. **No behavior change.** No encoded byte may move. The gates enforce it;
   don't rationalize around them.
9. **Overflow.** If step 2 cannot finish this session, stop at a whole-closure
   boundary, run the step-3-onward close on what is done ONLY if the §7
   conditions in step 4 can all be met; otherwise leave the phase open,
   enumerate the shortfall, and end the report with "session J scope: …".
   Do not weaken a condition to close.

## Current state — verified facts you will act on

- **`pPSOVector`** (`encoder/encoder_context.rs`, a context field): dead.
  `grep -rn 'pPSOVector' --include='*.rs' src/` → exactly 4 hits: the field
  declaration, the `new()` initializer, and the equality-instrument's field
  list. Never read, never written elsewhere.
- **`pFuncList`** (`*mut SWelsFuncPtrList`): the context's last unowned member.
  Allocated at `encoder/encoder_ext.rs:1448` and `:1620` (two sites), freed at
  `:1817`. These are 3 of the 15 remaining allocator hits. One
  `mem::zeroed` `Default` in `encoder/wels_func_ptr_def.rs`. Existing tests
  that already pin the table's initialization:
  `init_fills_sad_and_satd_and_clears_combined3`,
  `init_fills_every_slot_the_md_layer_indexes`,
  `init_fills_every_reconstruction_slot`.
- **The tables are re-written mid-stream** — this is why readers cannot hold
  `&SWelsFuncPtrList` across frames: `SetFastCodingFunc`
  (`encoder/encoder_ext.rs:2233`) and `SetNormalCodingFunc` (`:2243`) are
  called per frame at `:2298`/`:2300`; `WelsInitSampleSadFunc` is called from
  `encoder/encoder_context.rs:1457` and `encoder/svc_mode_decision.rs:2444`.
  Readers copy `Option<fn>` values out of the table; none holds a pointer into
  it across calls. Session E built a type-driven checker for exactly this
  claim — run it; if it finds a cross-call holder, that site stays raw and
  goes on the survivor list.
- **`*mut SWelsFuncPtrList` parameter sites**: 57.
- **`*mut sWelsEncCtx`**: 297 lines total. By file: rc.rs 55,
  svc_encode_slice.rs 50, ref_list_mgr_svc.rs 32, encoder_context.rs 31,
  encoder_ext.rs 28, svc_mode_decision.rs 21, svc_base_layer_md.rs 15,
  wels_preprocess.rs 13, **wels_task_management.rs 12 — Phase 7's file, do not
  touch**, **wels_encoder_ext.rs 8 — Phase 8's file, do not touch**, remainder
  in smaller files. Conversion surface ≈270 lines.
- **Known cross-call cursor holders** (will stay raw parameters, category 2):
  the NAL-writer chain over the frame bitstream — session H enumerated its
  **19** cursor derivations (list in H's log entry).
- **Allocator hits**: 15 = 4 Phase 7 (`DynamicSliceBs` at
  `encoder_ext.rs:1122`/`:1767`; `sSliceBs.pBs` at
  `svc_encode_slice.rs:2982`/`:3011`) + 3 this session (`pFuncList`, above) +
  8 dormant screen-content (`svc_motion_estimate.rs`, tagged
  `SCREEN_CONTENT(dormant: Phase 10)`).
- **`mem::zeroed`**: 9 sites — wels_func_ptr_def 1 (dies in step 1),
  svc_encode_slice 2, ref_list_mgr_svc 1, decode_mb_aux 1,
  get_intra_predictor 3, sample 1 (POD `Default`s and test fixtures — each
  either keeps a one-line soundness comment or converts if its type gains an
  owned field).
- **Modules**: 32 files in `src/encoder`, plus `src/processing`. None carries
  `deny(unsafe_code)` yet. The idiom to copy is the decoder's:
  `#![deny(unsafe_code)]` as an inner attribute at module top (see
  `decoder/decoder_core.rs:43`), `#[allow(unsafe_code)]` on each surviving
  item.
- **The four lawful allow categories** (phase6.md §7, condition 2 — every
  surviving `#[allow(unsafe_code)]` item must be one of these, tagged in a
  comment on the item):
  - `C-ABI` — values crossing the C ABI (owner: Phase 8)
  - `cursor` — owned-storage cursor machinery carrying its derive-twice
    Miri tests (the frame-bitstream writers, pool accessors, the named
    neighbour-walkers)
  - `MT` — a multi-threading seam (owner: Phase 7)
  - `SCREEN_CONTENT(dormant)` — the fenced screen-content family
    (owner: Phase 10)

## Step 0 — delete `pPSOVector`

1. Delete the field, its `new()` line, and its row in the equality instrument.
2. Re-measure and re-pin `assert_size!(sWelsEncCtx, …)` and every
   `assert_ctx_offset!` (they all shift). One commit.

Accept: `grep -rn 'pPSOVector' --include='*.rs' src/` → 0 hits;
`gates.sh commit` green.

## Step 1 — `pFuncList` becomes `Box<SWelsFuncPtrList>`; the tables re-spell

1. Field-wise constructor for `SWelsFuncPtrList` replacing the zeroed
   `Default`; the `init_fills_*` tests must stay green unmodified (they are the
   proof).
2. Both alloc sites become `Box::new(...)` through the constructor path; the
   free at `:1817` is deleted (the `Box` drops with the context).
3. Parameter re-spelling: `SetFastCodingFunc`, `SetNormalCodingFunc`,
   `WelsInitSampleSadFunc` take `&mut SWelsFuncPtrList`. Every reader takes
   `&SWelsFuncPtrList`, derived fresh at each call from the context's `Box`.
   Run session E's type-driven checker first; any cross-call holder it finds
   stays `*mut` and goes on the survivor list with the checker's output quoted.

Accept:
`grep -rn 'WelsMalloc\|WelsMallocz\|WelsFree' --include='*.rs' src/encoder src/processing`
→ exactly the 4 Phase-7 hits + the 8 dormant;
`grep -rn '\*mut SWelsFuncPtrList' --include='*.rs' src/encoder src/processing`
→ 0 outside the enumerated survivors;
`gates.sh family` green; `MIRI_SCOPE=encoder` Miri green.

## Step 2 — context parameters, root-down

Procedure, per function on the ≈270-line surface, starting from the api entries
(`WelsInitEncoderExt`, `WelsEncoderEncodeExt`, and their callers in
`wels_encoder_ext.rs`, which keep their raw handle — the reference is born at
that boundary, derived from the owner once per call) and descending
closure-by-closure (a reachability closure of signatures is one commit):

Ask two questions of the function body:
- **Q1**: does it hold a context-derived raw cursor across a call that reaches
  the context again?
- **Q2**: does it reach the same object through two of its parameters?

If both answers are no → the parameter becomes `&mut sWelsEncCtx` (or
`&sWelsEncCtx` if it only reads). If either is yes → the parameter stays
`*mut`, and the function goes on the **survivor list** with a one-line blocker:
`fn name — Q1: <the cursor and the call>` or `Q2: <the two paths>`, plus its
category (`cursor` / `MT` / `C-ABI`).

Do not edit `wels_task_management.rs` or `wels_encoder_ext.rs`; their lines are
pre-attributed survivors.

Accept: `grep -rn '\*mut sWelsEncCtx' --include='*.rs' src/encoder src/processing`
→ only the survivor list + the two untouched files, and the list is in the log;
`gates.sh family` green; `MIRI_SCOPE=encoder` Miri green.

## Step 3 — the deny sweep

1. Module by module, smallest first: add `#![deny(unsafe_code)]` at the top
   (decoder idiom), build, and put `#[allow(unsafe_code)]` + a category tag
   comment on every item the compiler names. An `unsafe fn` whose signature
   names a raw pointer is *correctly* `unsafe fn` — allow and tag it; do not
   force-convert.
2. The two MT files get no deny — instead a module-top comment:
   `// deny(unsafe_code) lands with Phase 7; this file is the thread machinery.`
3. Record per file: allow-item count per category. Re-baseline the ratchet
   (`bash rust/tools/unsafe_ratchet.sh` writes the new baseline) and explain
   every delta in the commit.

Accept: `grep -rln 'deny(unsafe_code)' src/encoder src/processing | wc -l` =
module count − 2; every allow item greps with a category tag on the same or
preceding line; `gates.sh family` green.

## Step 4 — the exit conditions and the handoffs

Check phase6.md §7's five conditions one by one and write the evidence into the
log entry:

1. deny on every module except the enumerated MT files → the step-3 grep.
2. every allow item in one of the four categories → the step-3 table.
3. encoder-side `raw_ptr` residue enumerated by category, code split from prose
   → run `bash rust/tools/unsafe_ratchet.sh` and the per-family greps; produce
   the table.
4. full battery PASS + cumulative perf restated → step 5.
5. handoffs written → edit `rust/docs/safety_refactor_plan.md` §4, adding to
   each phase row (verify each item against the tree before writing it):
   - **Phase 7**: F61; F3 ablation (85 measurements, the short-stream shape);
     F12/thread pool; `sSliceBs.pBs` + `DynamicSliceBs` + the MT context
     members + both MT files; `pMemAlign` and `common/memory_align.rs`'s
     retirement; E's one MT `*mut SMB` site.
   - **Phase 8**: `wels_encoder_ext.rs` internals; the `c_void` C-ABI line;
     note that `pSvcParam` is context-owned (H's verdict) so it is NOT
     inherited.
   - **Phase 9**: SAD/SATD third park (1.30–5.68x / 1.36–4.05x); `SMbCache`'s
     72 kernels-take-slices sites; F5's ~10 side-array resolutions; H's
     Vec-vs-`Option<Box>` accessor cost ({LTR, rc, ref lists},
     `WelsRcMbInitGom`/`ctx_rc_at` named); D-perf-6's recovery; the two
     `pMvdCost` record fields.
   - **Phase 10**: the `SCREEN_CONTENT(dormant)` census (16 tagged items + 8
     allocator hits).

## Step 5 — the close

1. The span (hard rule 3), recorded in `rust/docs/perf_baseline.md`.
2. The full battery: `bash rust/tools/gates.sh exit` **without** `MIRI_SCOPE`
   — the unscoped Miri `--lib` step must show all four encoder probes AND all
   three decoder probes green by name. Adjudicate any F3 hit per hard rule 7.
3. The phase-close log entry in `rust/docs/safety_refactor_log.md`: Phase 6's
   arc in numbers — encoder-side `raw_ptr` 2668 (phase open) → final,
   `unsafe_fn` 710 → final, allow items per category, findings F57–F64 with
   dispositions, sweep growth 341 → 369, the per-session span list, cumulative
   perf vs both perf decisions.
4. `rust/docs/safety_refactor_plan.md` §0: write the **Phase 6 COMPLETE** row
   (model it on the existing "Phase 5b COMPLETE" row: what closed, the
   numbers, what each later phase inherits), mark session I spent, name
   **Phase 7** next. Update phase6.md §6's I row to SPENT with the same
   summary. Commit as `docs(T6.I-close): …`.

## Do not touch

| what | owner |
|---|---|
| `slice_multi_threading.rs`, `wels_task_management.rs`, MT context members, `sSliceBs.pBs`, `DynamicSliceBs`, `pMemAlign`, `common/memory_align.rs`, `common/wels_thread_pool.rs` | Phase 7 |
| `wels_encoder_ext.rs` internals | Phase 8 |
| SAD/SATD kernels, `SMbCache`'s 72 kernel sites, `*mut SMB`'s 48 named survivors, per-MB plane cursors, the two `pMvdCost` fields, the {LTR, rc, ref-list} accessor cost | Phase 9 |
| anything tagged `SCREEN_CONTENT(dormant: Phase 10)` | Phase 10 |

## Report back, in this order

1. One line: phase closed or not; `exit` verdict; tree state and HEAD.
2. The five §7 conditions, each with its evidence one-liner.
3. `*mut sWelsEncCtx` before → after, with the survivor count and its category
   split; same for `*mut SWelsFuncPtrList`.
4. Deny/allow numbers: modules denied, allow items per category.
5. The span and the cumulative restatement.
6. Anything found and not fixed, with owner.
7. If the phase did not close: "session J scope:" with the enumerated remainder.
