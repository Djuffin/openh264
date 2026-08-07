# Session prompt — Safety refactor, Phase 0: guardrails, dead-code purge, tooling

You are executing **Phase 0** of the plan in `rust/docs/safety_refactor_plan.md` (read it in full before doing anything; §Phase 0, §1.2 taxonomy, §1.4 safety nets, and §7 verification are the load-bearing sections). The plan is the canonical strategy document; this prompt is the operational brief for Phase 0 and reflects repo state as of `909c368b` — where they disagree, this prompt is newer, and part of your job is to update the plan when you find discrepancies.

**Mission:** make the refactor *measurable and safe to start* — baselines recorded, dead unsafe deleted, fuzzing and gate tooling stood up — while changing **zero** behavior: no decoded pixel, no encoded byte, no error code, no conformance frame count may move. Phase 0 contains no refactoring. Do not begin Phase 1 (the `src/safe/` vocabulary types) no matter how tempting.

---

## 1. Read first, in this order

1. `rust/docs/safety_refactor_plan.md` — the whole plan.
2. `git show fa67432f` and `git show 909c368b` — the `FillDefaultExt` defaults fix and the diffharness restore. You inherit these; understand what they changed.
3. `rust/tools/diffharness/{build.sh,compare.sh,sweep.sh}` and `cxx_enc.cpp` — the encoder oracle you will gate with. Note `git status` shows an **uncommitted modification to `sweep.sh`** — task T2 deals with it; look at the diff before running sweeps.
4. `git show eb463dbd^:rust/docs/encoder_port_handoff.md` — orientation from the sessions that built the port. **Treat its claims as unverified** (it says so itself, and it has been wrong). It documents harness traps you need — including a zsh word-splitting bug that silently feeds garbage arguments to sweep runs; run the sweep scripts with `bash` unless the recovered docs say otherwise.

Facts you would otherwise rediscover the hard way:

- The conformance harness **drops frames silently on a Rust decoder error** — when comparing, check frame counts before hashes, always.
- The C++ dylib used in benches runs **scalar only** (NEON linked, never dispatched) — every C++-vs-Rust perf number is scalar-vs-scalar.
- JVT/ffmpeg golds cannot judge B-slice streams (upstream C++ itself diverges there); the 13 `#[ignore]`d e2e tests are permanent fixtures. Their set must not change.
- The `def` sweep preset (GetDefaultParams mode) exists as of `909c368b`; drivers take `baseinit=2` in that mode. It is part of the gate battery.

## 2. State you inherit

- `278eb399` — the safety plan committed. `fa67432f` — `FillDefaultExt` was missing `bFixRCOverShoot = true` and `iIdrBitrateRatio = 400` vs C++ `param_svc.h:181-182`; fixed and verified byte-identical on four sequences up to 720p, plus `sweep.sh st mt` 330/330 and `def` 11/11.
- **The `eb463dbd` release-build segfault** (null `(*pCurMb).pNonZeroCount` in `encoder::deblocking::WelsNonZeroCount_c`, `deblocking.rs:873`, via `pfSetNZCZero` from `DeblockingBSCalcEnc`; debug ran the same input 200 frames clean) **no longer reproduces at HEAD — but was never root-caused**, and nothing in `fa67432f`/`909c368b` plausibly touches that pointer wiring. This is an open UB question over the codebase; task T3 owns it.
- `rust/docs/` contains only the plan; the deleted handoff/status docs live at `eb463dbd^:rust/docs/`.
- Working tree: `sweep.sh` modified, uncommitted. Everything else clean.

## 3. Ground rules

- **Behavior-neutral, provably.** Every commit must pass the per-commit gates (§5). If a gate fails after a deletion, `git revert`/reset that deletion — do not debug forward through a red gate in this phase.
- **Verify "dead" before deleting.** The plan's line numbers were verified 2026-08-07 but drift; the *claims* ("referenced zero times") must be re-proven by grep at deletion time, per the recipes in T5. If a claim doesn't hold, skip that deletion, note it in the log, move on.
- **One logical unit per commit.** Match the existing log style: lowercase, imperative, optional `area:` prefix (`diffharness: restore from eb463dbd^ with a GetDefaultParams mode`). Never mix two deletion families in one commit — reverts must stay surgical.
- **No collateral improvement.** No renames, no formatting churn, no clippy fixes, no comment rewrites in files you pass through. The diff of every commit should read as exactly its stated purpose.
- **No new dependencies** in `openh264-rs` (`[dependencies]` stays `libc` only for now). The fuzz crate is separate and may depend on `libfuzzer-sys`.
- If you find a real bug that is not in this prompt's scope (fuzz finding, suspicious code while grepping): **record it** (T7's findings file), don't fix it. The only permitted behavior change in Phase 0 is a fix for the release segfault *if* T3 proves it still exists at HEAD.

## 4. Tasks, in order

Phase 0 is sized at 2–3 sessions. Suggested split: T1–T4 (session A), T5 (session B), T6–T8 (session C) — but take them strictly in order and stop cleanly wherever the session ends (§6). If a previous session already checked items off in the plan's Progress appendix, verify its end-state gates still pass and continue from the first unchecked item.

### T1 — Session-start control run
Run the full battery as it exists (§5, "full battery") and record every result in the session log (§6). This is your control; nothing may deviate from it later except the ratchet counts (down) and anything T3 explicitly fixes. If the control run itself is red anywhere, **stop and investigate before proceeding** — you may be inheriting a broken tree, and Phase 0 must start green.

### T2 — Resolve the `sweep.sh` working-tree modification
`git diff rust/tools/diffharness/sweep.sh`, understand it, then either commit it (with a message saying what it adds) or discard it (with the reason in the log). Do not leave it dangling — every later gate run must be reproducible from committed state.

### T3 — Time-boxed: the `eb463dbd` release-segfault question
Budget: **half a session, hard stop.** In a worktree (`git worktree add ../openh264-eb463dbd eb463dbd`), build the release diffharness and attempt to reproduce the crash with the original config (release `rust_enc`, the explicit-param gate configuration, 200-frame 320x240 input — the recovered handoff and the memory note describe the setup). Outcomes:

- **Reproduces at `eb463dbd`:** root-cause it there (the crash site and the T5 wiring suspicion are in plan §1.4). Then determine whether HEAD still contains the latent bug (code inspection of the wiring path + attempted repro at HEAD with the same config). If HEAD is affected: fixing it is in scope — smallest possible fix, its own commit, full battery before and after. If HEAD is clean: identify *which* commit fixed it and record that.
- **Does not reproduce:** record exactly what was tried (configs, inputs, run counts — UB can be nondeterministic, so do multiple runs) and close as not-reproducible-with-watch-note.

Either way, write the finding into: the plan's Progress appendix, the session log, and the auto-memory note about the segfault (`encoder-verified-only-in-debug-builds`) so future sessions inherit the answer instead of the question. Remove the worktree when done.

### T4 — Recover the deleted docs; record baselines
1. `git show eb463dbd^:rust/docs/encoder_port_handoff.md > rust/docs/encoder_port_handoff.md`, same for `encoder_port_status.md`; check `git show eb463dbd --stat -- rust/tools/ rust/docs/` for anything else worth recovering (audit scripts). Prepend a short header to each recovered doc: recovered from `eb463dbd^` on <date>, historical reference, claims unverified. One commit.
2. `rust/docs/perf_baseline.md`: for `c_vs_rust_bench` and `decode_1080p_bench`, 3 runs each, record per-row numbers and medians, plus machine info (`sysctl -n machdep.cpu.brand_string`, macOS version) and the scalar-vs-scalar caveat. This file is the §7.4 budget anchor for all later phases.
3. Conformance baselines are the test assertions themselves — no snapshot needed beyond the green control run recorded in T1.

### T5 — Dead-unsafe purge
One commit per family, per-commit gates between, in this order (safest first). For every deletion: **prove deadness first** with the recipe given; paste the proving grep output into the commit message or the log.

- **T5a — the 62 unreferenced SIMD extern declarations**, `common/mc.rs:793-863` (`_neon/_sse2/_ssse3/_mmx/_lsx` blocks). Recipe: for each declared symbol `S`, `grep -rnw "S" rust/crates/openh264-rs/src/` (`-w` = whole word; BSD-grep-safe) must hit only the declaration line. Spot-check a handful individually, then delete the whole block and let the compiler confirm (they're `extern` decls — unused ones vanish silently, so the grep *is* the proof).
- **T5b — SIMD delegating stubs + table re-pointing.** Families: `encoder/encode_mb_aux.rs:772-930` (`_sse2/_sse42/…` one-liners delegating to `_c`), `encoder/sample.rs`, `encoder/get_intra_predictor.rs`, `encoder/decode_mb_aux.rs`, `common/intra_pred_common.rs` (`_sse2/_neon/_AArch64_neon/_mmi/_lsx` — note the `_sse2` variant contains real `core::arch` intrinsics but is **never installed**: only the `_c` variants are wired, per `encoder/get_intra_predictor.rs:782-783`; it dies too, deliberately — future SIMD re-enters by design, not by resurrecting dead code), plus decoder-side equivalents. Recipe per family: find every table-init/installation site (`grep -rn "FamilyFnName" src/`), confirm only `_c` variants are ever installed, re-point any stub-referencing table entries to `_c`, delete the stubs. **Trap:** `common/intra_pred_common.rs` and `decoder/get_intra_predictor.rs` both define `WelsI16x16LumaPredV_c`-style names with *different arities* (3-arg vs 2-arg) — they are different functions; never unify or cross-delete.
- **T5c — decoder threading scaffolding** (the decoder has no threading; stubs only). Targets: `SWelsDecThreadInfo`/`SWelsDecoderThreadCTX` (`decoder_context.rs:791-821`), `SWelsDecEvent` (`picture.rs:84-90`) + `EventCreate/EventDestroy` (`pic_queue.rs:134-155`), the thread-gated allocations (`pic_queue.rs:276-283` `pNzc`, `:310-320` `pReadyEvent`), `OpenDecoderThreads`/`CloseDecoderThreads` no-ops (`decoder_core.rs:2014-2027`) and their call sites, `GetThreadCount` (`decoder_core.rs:770-776`) simplified to return 1 (keep the function — its call sites stay honest). Recipe: first prove `pThreadCtx`/`pLastThreadCtx`/`pCsDecoder` are never assigned non-null (`grep -rn "pThreadCtx" src/decoder/ src/api/`); then, for each field you want to remove (`pReadyEvent`, decoder-`Picture::pNzc`), grep all reads — delete the field only if every read is inside code you're deleting or behind an always-false thread-count gate. **If any read is ambiguous, delete only the dead branches and leave the field with a `// dead: decoder MT was never ported (plan §Phase 0)` note for Phase 5.** Conservative beats clever here.
- **T5d — duplicated bitstream writer.** `svc_encode_slice.rs:498-585` duplicates `BsGetBitsPos`/`BsWriteBits`/`BsWriteOneBit`/`BsWriteUE`/`BsWriteSE` from `vlc_encoder.rs`. Recipe: diff the two copies function-by-function first — they must be semantically identical (the survey says verbatim; verify). If identical: re-point the duplicate's callers at `vlc_encoder.rs`, delete the copy. If they differ *at all*: stop, record the difference (that's a latent divergence bug), don't dedupe in this phase.
- **T5e — small stragglers:** dead `use std::ffi::c_void` in `lib.rs`; anything T5a–d exposed as newly-unreferenced (compiler warnings will tell you — but remember `dead_code` is `allow`ed crate-wide, and in-source `allow` attributes override plain `-W` flags — use `RUSTFLAGS="--force-warn dead_code" cargo build 2>&1 | tee` once (`--force-warn` is the only level that beats attributes) and harvest, deleting only what belongs to the families above).
- **Do NOT delete:** `decoder_decode_frame_ex_c` / `decoder_decode_parser_c` (real upstream vtable slots — drop-in ABI, plan §2.2.8), anything in `api/codec_api.rs`, the version functions, or any `#[repr(C)]` struct.

After T5, regenerate ratchet counts (T6 script, if you built it early) or record raw greps: totals must be strictly below T1's.

### T6 — Ratchet script + gate runner
1. `rust/tools/unsafe_ratchet.sh`: per-file counts over `rust/crates/openh264-rs/src/` of: `unsafe fn`, `unsafe {`, `\*mut |\*const `, `transmute`, `unsafe impl`, `mem::zeroed`, `no_mangle`, `SHIM(`. Two modes: `generate` writes `rust/tools/unsafe_baseline.json` (sorted, stable ordering for clean diffs); `check` recounts and exits non-zero listing any file×metric that **increased** vs baseline (decreases are fine and expected). Keep it plain POSIX-ish shell + standard tools; no jq dependency if easy to avoid.
2. `rust/tools/gates.sh`: one command running the whole battery (§5 "full battery"), printing a PASS/FAIL line per gate and a summary, non-zero exit on any failure. Sweep steps must invoke the diffharness the documented-safe way (bash; watch the word-splitting trap).
3. Commit the scripts + the generated baseline (post-T5 counts). Document both in the plan (§7.1/§7.5 already describe them — add the actual invocations).

### T7 — Fuzz crate
1. `cargo fuzz init` inside `rust/crates/openh264-rs/` (cargo-fuzz's conventional `fuzz/` subdir; the plan's `rust/fuzz/` shorthand means this — note the concrete path in the plan when you commit). Requires nightly (`cargo +nightly fuzz …`); record the toolchain in the fuzz README.
2. Target `decode_annexb`: drive the decoder exactly like `tests/decoder_conformance_test.rs` does — create via `WelsCreateDecoder`, `Initialize` with the conformance `SDecodingParam` (`ERROR_CON_SLICE_COPY`), split input with `split_annexb_units`, `DecodeFrame2` per unit, then the EOS + flush drain sequence, `WelsDestroyDecoder`. Input: the raw fuzz bytes as the whole bitstream. The fuzzer's own panic/crash/timeout detection is the oracle — the target asserts nothing beyond "returns".
3. Seed corpus: the `res/` JVT streams (symlink or copy the `.264` files into `fuzz/corpus/decode_annexb/`).
4. Burn-in: 30–60 minutes locally. **Findings are pre-existing bugs, not your bugs to fix now**: commit each crash artifact under `fuzz/regressions/`, describe it in `rust/docs/fuzz_findings.md` (input, panic site, one-line hypothesis, which phase should fix it — bitstream panics → Phase 3, picture/DPB → Phase 5), and move on. The plan's P13 (no panics on malformed input) is an end-state goal, not a Phase 0 gate.
5. Wire a cheap corpus-regression step into `gates.sh` (`cargo +nightly fuzz run decode_annexb -- -runs=0` over the corpus — replay only, no exploration) so later phases can't regress on known inputs. Gate it on nightly being available; skip with a loud SKIP line otherwise.

### T8 — Bookkeeping & handoff
1. Append a `## Progress` appendix to `rust/docs/safety_refactor_plan.md`: a per-phase checklist; check off Phase 0 items with commit hashes; record the T3 finding verbatim.
2. Create `rust/docs/safety_refactor_log.md`: per-session entries — date, session goal, what landed (commits), gate results (control vs final), open questions, next session's first action. Write this session's entry. Every later phase session appends here.
3. Update auto-memory: the segfault note per T3's outcome; extend the safety-refactor-plan memory with "Phase 0 done/partial as of <commit>, gates runner is `rust/tools/gates.sh`".
4. End state: clean `git status`, full battery green, ratchet baseline strictly below the session-start counts.

## 5. Gates for this phase

**Per-commit gates** (fast, run before every commit):
- `cargo test` and `cargo test --release` in `rust/crates/openh264-rs/` — all green, `#[ignore]` set unchanged (both profiles matter: the release history is exactly why).

**Family-checkpoint gates** (after each T5 family, and after T3 if it changed anything):
- Per-commit gates + diffharness `bash sweep.sh st`, `mt`, `def` — all runs byte-identical, run counts equal to T1's control (e.g. if control said 330/330, a 329/330 with one "identical" missing is a **failure**, not a rounding error).

**Full battery** (T1 control, and session end):
- Everything above, both build profiles for the harness, + bench smoke run (one pass each bench, sanity vs `perf_baseline.md` — not a budget gate yet, Phase 0 sets the baseline) + `unsafe_ratchet.sh check` (once it exists) + fuzz corpus replay (once it exists).

**On any gate failure after a change:** revert the change first, confirm green, then think. The one exception is T1's control run — if *that* is red, nothing proceeds until you understand why.

## 6. Ending a session (including early)

Never leave: uncommitted mixed work, a red gate, or an unrecorded decision. If time runs out mid-task: finish or revert the current commit-sized unit, run per-commit gates, write the log entry with "next first action", and stop. A later session must be able to resume from the log + Progress appendix alone.

## 7. Explicit non-goals for Phase 0

No Phase 1 vocabulary types. No `unsafe`→safe conversions. No signature changes. No `unsafe_op_in_unsafe_fn` flip (plan §7.1 explains why not). No renaming, no reformatting, no clippy, no CI configuration (local scripts only). No fixing fuzz findings. No touching `api/codec_api.rs`, the vtable slots, or anything `#[repr(C)]`. No dependency changes in the main crate. No edits under `codec/` (the C++ is the reference; it stays pristine — the diffharness builds it, you never patch it).
