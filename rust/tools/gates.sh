#!/bin/bash
# The gate battery — plan §7.2, one command.
#
#   usage: rust/tools/gates.sh [level]
#
#     commit   build --all-targets + cargo test (debug + release) + ratchet (~2 min)
#     family   commit + diffharness sweeps st/mt/def/sl in BOTH profiles  (~5 min)
#     full     family + decode bench + encoder bench + Miri --lib         (default)
#     exit     full + Miri over the differential integration tests        (phase exits)
#
#   env:
#     FFMPEG=/path/to/ffmpeg   required for the encoder bench; without it that
#                              gate SKIPs loudly (the fallback measures the
#                              frame-skip path, not encoding — perf_baseline.md §2)
#     BENCH_ITERS=3            passes per bench row
#     SKIP_SWEEP=1             drop the sweep gates (use only when you know why)
#
# Written as bash on purpose, like the diffharness: the interactive shell here is
# zsh, which does not word-split unquoted expansions, and the sweep scripts rely
# on that splitting. Run this with `bash`, never `sh` or `zsh`.
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
CRATE="$ROOT/rust/crates/openh264-rs"
DIFF="$HERE/diffharness"
LOGS="$ROOT/.gates"                # scratch; gitignored
LEVEL=${1:-full}

case "$LEVEL" in
  commit|family|full|exit) ;;
  *) sed -n '2,20p' "$0"; exit 2 ;;
esac

mkdir -p "$LOGS"
PASS=0; FAIL=0; SKIP=0
RESULTS=()

pass() { PASS=$((PASS+1)); RESULTS+=("PASS  $1"); printf 'PASS  %s\n' "$1"; }
fail() { FAIL=$((FAIL+1)); RESULTS+=("FAIL  $1"); printf 'FAIL  %s\n' "$1"; }
skip() { SKIP=$((SKIP+1)); RESULTS+=("SKIP  $1"); printf 'SKIP  %s\n' "$1"; }

hdr() { printf '\n=== %s\n' "$1"; }

# ---------------------------------------------------------------------------
# 0. Every target compiles (Phase 5 session U, T5.T5).
#
# `cargo test` builds the lib, the unit tests, the integration tests and the
# bins — and NOT the benches or the examples. So a change to a *public* type can
# break a bench and leave the whole per-commit gate green: that is exactly what
# T5.T1's `DECODING_STATE` newtype did, and nothing said so until the `exit`
# battery tried to run `decode_1080p_bench` five commits later and it did not
# compile. Five commits of that session touched a public type.
#
# The cost is seconds (it shares the target dir with the tests that follow, so
# almost everything here is a cache hit), which is why this closes the hole
# mechanically rather than by a rule telling sessions to remember.
#
# Debug only, deliberately: the missing targets are the same set in both
# profiles and a type error is profile-independent, so the release compile is
# `run_cargo_test release`'s to pay for. `--all-targets` implies `--benches`,
# which *builds* the benches without running them.
# ---------------------------------------------------------------------------
hdr "cargo build --all-targets"
(cd "$CRATE" && cargo build --all-targets 2>&1) | tee "$LOGS/build_all_targets.log" | grep -E '^error|^warning: unused|Compiling|Finished' | tail -15
if [ "${PIPESTATUS[0]}" -ne 0 ]; then
  fail "cargo build --all-targets: a target does not compile — see $LOGS/build_all_targets.log"
else
  pass "cargo build --all-targets: benches and examples compile"
fi

# ---------------------------------------------------------------------------
# 1. cargo test, both profiles.
#
# The counts matter as much as the exit status. A decoder error silently drops
# frames rather than failing a test, and a test that stops being compiled in
# looks exactly like a test that passes. So: totals AND the ignored count, which
# is a permanent fixture set of 20 (plan §1.4) and must never move.
# ---------------------------------------------------------------------------
run_cargo_test() {  # $1 = profile label, $2.. = extra cargo args
  local label=$1; shift
  local log="$LOGS/cargo_test_$label.log"
  hdr "cargo test ($label)"
  (cd "$CRATE" && cargo test "$@" 2>&1) | tee "$log" | grep -E '^test result:|^error|^warning: unused' | tail -40
  local rc=${PIPESTATUS[0]}
  local passed failed ignored
  passed=$(awk '/^test result:/ { for (i=1;i<=NF;i++) if ($i=="passed;") s+=$(i-1) } END { print s+0 }' "$log")
  failed=$(awk '/^test result:/ { for (i=1;i<=NF;i++) if ($i=="failed;") s+=$(i-1) } END { print s+0 }' "$log")
  ignored=$(awk '/^test result:/ { for (i=1;i<=NF;i++) if ($i=="ignored;") s+=$(i-1) } END { print s+0 }' "$log")
  printf '  totals: %s passed / %s failed / %s ignored\n' "$passed" "$failed" "$ignored"
  if [ "$rc" -ne 0 ] || [ "$failed" -ne 0 ]; then
    fail "cargo test ($label): $passed/$failed/$ignored"
  elif [ "$ignored" -ne 20 ]; then
    fail "cargo test ($label): ignored set is $ignored, must be 20 (plan §1.4)"
  else
    pass "cargo test ($label): $passed passed / 0 failed / 20 ignored"
  fi
}

run_cargo_test debug
run_cargo_test release --release

# ---------------------------------------------------------------------------
# 1b. Miri, same verdict discipline (F17, phase3_findings.md).
#
# Both Miri steps used to be written `if (cargo miri …) | tee … | tail -5; then`.
# A pipeline's exit status is its LAST command's, this script does not set
# `pipefail` (deliberately — see the audit note at the bottom), and `tail`
# succeeds whenever it can write. So the `if` never saw cargo's status: every
# `PASS  miri` printed between Phase 2's exit and 2026-08-10 was unconditional.
# The gate was a reporter, not a gate.
#
# The fix is `run_cargo_test`'s, which had it right all along: keep the `tail`
# for display, take the verdict from `${PIPESTATUS[0]}`, and corroborate it by
# parsing libtest's own totals out of the log. The zero-test clause is not
# belt-and-braces — it is this script's own header comment applied to Miri ("a
# test that stops being compiled in looks exactly like a test that passes"), and
# a mistyped `--skip` or a renamed module walks straight through everything else.
# ---------------------------------------------------------------------------
run_miri() {  # $1 = log slug, $2 = description, $3 = extra MIRIFLAGS, $4.. = args to `cargo miri test`
  local slug=$1 desc=$2 mflags=$3; shift 3
  local log="$LOGS/miri_$slug.log"
  (cd "$CRATE" && MIRIFLAGS="${MIRIFLAGS:-} $mflags" \
     cargo +nightly miri test "$@" 2>&1) | tee "$log" | tail -5
  local rc=${PIPESTATUS[0]}
  local passed failed
  passed=$(awk '/^test result:/ { for (i=1;i<=NF;i++) if ($i=="passed;") s+=$(i-1) } END { print s+0 }' "$log")
  failed=$(awk '/^test result:/ { for (i=1;i<=NF;i++) if ($i=="failed;") s+=$(i-1) } END { print s+0 }' "$log")
  printf '  totals: %s passed / %s failed   (cargo miri rc=%s)\n' "$passed" "$failed" "$rc"
  if [ "$rc" -ne 0 ] || [ "$failed" -ne 0 ]; then
    fail "miri $desc: $passed passed / $failed failed, rc=$rc — see $log"
  elif [ "$passed" -eq 0 ]; then
    fail "miri $desc: ran 0 tests (rc=0) — nothing was compiled in; see $log"
  else
    pass "miri $desc: $passed passed / 0 failed"
  fi
}

# ---------------------------------------------------------------------------
# 2. The unsafe ratchet (plan §7.1).
# ---------------------------------------------------------------------------
hdr "unsafe ratchet"
if bash "$HERE/unsafe_ratchet.sh" check; then
  pass "unsafe ratchet: no per-file increase"
else
  fail "unsafe ratchet: a file x metric increased"
fi

# ---------------------------------------------------------------------------
# 2b. The duplicate census (plan §7.2, added Phase 5 session A).
#
# Three instruments, zero-or-allowlisted: duplicate declarations, type laundering
# by double cast, and duplicate function bodies. It runs at `commit` level because
# a second declaration of one entity is cheap to introduce and expensive to find
# later — F21 was a drifted third copy of one C++ function, F22 a second copy that
# dropped a null guard, and neither moved a byte on any stream this project owns.
# The allowlist is rust/tools/census_allowlist.txt and it carries the reason and
# the owning phase step per entry.
# ---------------------------------------------------------------------------
hdr "duplicate census"
if bash "$HERE/census.sh" > "$LOGS/census.log" 2>&1; then
  pass "duplicate census: $(grep -cE '^(type|alias|table|budget) ' "$HERE/census_allowlist.txt") allowlisted, nothing new"
else
  sed -n '1,60p' "$LOGS/census.log"
  fail "duplicate census: a new duplicate or a budget increase — see $LOGS/census.log"
fi

[ "$LEVEL" = commit ] && { LEVEL_DONE=1; }

# ---------------------------------------------------------------------------
# 3. Diffharness sweeps, both build profiles.
#
# F3 (phase0_findings.md) — READ BEFORE CALLING A SWEEP FAILURE A REGRESSION:
# roughly 1 in 400-1000 `mt` `sm=3` encodes produces a wrong bitstream (zero-byte
# or short), pre-existing, unfixed, a race in slice-list growth.
#
# The signature is any `mt` configuration at `sm=3`, `t` in {2,4}, in EITHER
# profile. Debug used to be exempt (0-in-1200) and no longer is: that immunity was
# an artefact of `opt-level = 0` being too slow to lose the race, and the driver now
# builds at `opt-level = 3` (F3's fourth measurement, 2026-08-10). The checks are
# still on — only the speed changed.
#
# One such failure is F3 until proven otherwise: re-run that configuration. If more
# than one lands in a session, prove the rate by ALTERNATING HEAD and the control
# commit inside one loop — sequential sampling of a load-sensitive race is not a
# comparison — and append the measurement to F3.
#
# A failure at ANY other configuration, or in `st`/`def`, is real, in either
# profile: stop, revert, investigate.
# ---------------------------------------------------------------------------
sweep_gate() {  # $1 = profile
  local prof=$1 log="$LOGS/sweep_$1.log" rc t0 t1
  hdr "diffharness sweep st mt def sl ($prof)"
  if ! RUST_ENC_PROFILE="$prof" bash "$DIFF/build.sh" > "$LOGS/build_$prof.log" 2>&1; then
    fail "sweep ($prof): harness build failed — see $LOGS/build_$prof.log"
    return
  fi
  t0=$(date +%s)
  RUST_ENC_PROFILE="$prof" bash "$DIFF/sweep.sh" st mt def sl 2>&1 | tee "$log" | tail -20
  rc=${PIPESTATUS[0]}
  t1=$(date +%s)
  local tally wall=$((t1 - t0))
  # sweep.sh prints `PASS=n FAIL=n` unconditionally as its last act before
  # exiting, so a missing tally means it died on the way there — a broken
  # instrument, not a verdict. It used to fall back to `tail -3` as display
  # text, which reads like a result. Corroboration, exactly as run_cargo_test
  # corroborates rc with the test totals.
  tally=$(grep -E '^PASS=[0-9]+ FAIL=[0-9]+' "$log" | tail -1)
  printf '  %s   (%ss wall, sweep.sh rc=%s)\n' "${tally:-<no tally line>}" "$wall" "$rc"
  if [ -z "$tally" ]; then
    tail -3 "$log"
    fail "sweep ($prof): no 'PASS=n FAIL=n' tally in $log (sweep.sh rc=$rc) — it died before finishing; this is a broken run, not an F3 hit"
  elif [ "$rc" -eq 0 ]; then
    pass "sweep ($prof): $tally, ${wall}s wall"
  else
    echo "  --- F3 retry rule ---"
    echo "  A single mt failure at sm=3, t in {2,4} (zero-byte or short output) is F3,"
    echo "  not a regression, in EITHER profile — debug included since the driver went"
    echo "  to opt-level 3. Re-run that configuration. st/def, or any other mt config,"
    echo "  is real. More than one hit: alternate HEAD and control in ONE loop."
    grep -E '^  ' "$log" | grep -vE '^\s*$' | head -10
    fail "sweep ($prof): $tally — apply the F3 retry rule above"
  fi
}

if [ "${LEVEL_DONE:-0}" != 1 ]; then
  if [ "${SKIP_SWEEP:-0}" = 1 ]; then
    skip "diffharness sweeps (SKIP_SWEEP=1)"
  elif [ ! -f "$ROOT/libopenh264.a" ]; then
    skip "diffharness sweeps: no libopenh264.a — run 'make -j8 libraries binaries' at the repo root"
  else
    sweep_gate debug
    sweep_gate release
  fi
fi

[ "$LEVEL" = family ] && LEVEL_DONE=1

# ---------------------------------------------------------------------------
# 4. Benches. The decoder bench is the sharper instrument (perf_baseline.md);
#    it exits non-zero on a frame-count or hash mismatch, which is the gate.
#
# Verdict shape (F17): three conditions, all of which must hold.
#   [0] cargo's own status — the one the old `if pipeline; then` discarded. A
#       bench that prints clean rows and then dies mid-run used to pass.
#   [2] the display grep matched something — this is the signal the old form
#       *did* carry (nothing matched ⇒ the bench produced no output at all), and
#       moving the verdict to PIPESTATUS[0] would have thrown it away.
#   the log's own MISMATCH/DIFFER marker, which was already checked.
# ---------------------------------------------------------------------------
if [ "${LEVEL_DONE:-0}" != 1 ]; then
  hdr "decode_1080p_bench"
  (cd "$CRATE" && BENCH_ITERS="${BENCH_ITERS:-3}" cargo bench --bench decode_1080p_bench 2>&1) \
    | tee "$LOGS/decode_bench.log" | grep -E 'Rust :|C\+\+  :|ratio:|MISMATCH|frames'
  # One capture, then index it. `a=${PIPESTATUS[0]}` is itself a command, so it
  # replaces PIPESTATUS with its own one-element status before a second
  # `${PIPESTATUS[2]}` can be read — which under `set -u` aborts the whole
  # battery. Found by the known-red self-test, on its first run.
  bench_st=("${PIPESTATUS[@]}")
  bench_rc=${bench_st[0]}; bench_matched=${bench_st[2]}
  if [ "$bench_rc" -ne 0 ]; then
    fail "decode_1080p_bench: non-zero exit (rc=$bench_rc) — see $LOGS/decode_bench.log"
  elif [ "$bench_matched" -ne 0 ]; then
    fail "decode_1080p_bench: exit 0 but printed no result rows — see $LOGS/decode_bench.log"
  elif grep -q 'MISMATCH' "$LOGS/decode_bench.log"; then
    fail "decode_1080p_bench: output mismatch"
  else
    pass "decode_1080p_bench: all streams bit-identical"
  fi

  hdr "c_vs_rust_bench (encoder)"
  # Find ffmpeg rather than requiring it to be exported. Phase 2 sessions A and B both
  # ran the whole battery with FFMPEG unset on a machine that had ffmpeg on PATH the
  # whole time, so the encoder went UNMEASURED across three families and T5's +16.8%
  # regression reached a commit before anything noticed. An instrument that silently
  # skips because of an unset variable is not an instrument.
  if [ -z "${FFMPEG:-}" ] && command -v ffmpeg >/dev/null 2>&1; then
    FFMPEG="$(command -v ffmpeg)"
    echo "  (FFMPEG was unset; using $FFMPEG from PATH)"
  fi
  if [ -z "${FFMPEG:-}" ]; then
    echo "  *** No ffmpeg on PATH and FFMPEG is not set. Without it this bench encodes"
    echo "  *** a synthetic pattern that the rate control skips from VGA up, so it"
    echo "  *** measures the cost of DECIDING TO SKIP rather than the encode kernels"
    echo "  *** (perf_baseline.md §2). Install ffmpeg, or set FFMPEG=/path/to/ffmpeg."
    skip "c_vs_rust_bench: no ffmpeg — encoder perf is UNMEASURED this run"
  else
    (cd "$CRATE" && FFMPEG="$FFMPEG" BENCH_REQUIRE_FFMPEG=1 \
       cargo bench --bench c_vs_rust_bench 2>&1) \
      | tee "$LOGS/encode_bench.log" | grep -E 'ms|IDENTICAL|DIFFER'
    bench_st=("${PIPESTATUS[@]}")
    bench_rc=${bench_st[0]}; bench_matched=${bench_st[2]}
    if [ "$bench_rc" -ne 0 ]; then
      fail "c_vs_rust_bench: non-zero exit (rc=$bench_rc) — see $LOGS/encode_bench.log"
    elif [ "$bench_matched" -ne 0 ]; then
      fail "c_vs_rust_bench: exit 0 but printed no rows — see $LOGS/encode_bench.log"
    elif grep -q 'DIFFER' "$LOGS/encode_bench.log"; then
      fail "c_vs_rust_bench: a row differed"
    else
      pass "c_vs_rust_bench: all rows bit-identical"
    fi
  fi

  # -------------------------------------------------------------------------
  # 5. Miri. `--lib safe::` on every full run: it is 20 seconds and it covers
  #    the vocabulary types. The differential integration tests go at phase
  #    exits only (~50s) — they are the ones that execute the OLD unsafe code,
  #    which is how F7 was found, so they are not optional, just infrequent.
  #    Sample counts scale down under cfg!(miri) via each file's scale() helper.
  # -------------------------------------------------------------------------
  # WIDENED at Phase 2's exit (2026-08-10) from `--lib safe::` to the whole
  # library. The old scope was the vocabulary types only, which meant a port unit
  # test exercising UB went unseen until a shim happened to materialise the same
  # span — session B flagged that gap, and widening it here found six real defects
  # in its first afternoon (three test-side accommodations, three in production
  # code; `phase2_findings.md` F10's third instance, F12 and F13).
  #
  # SKIPS. Each names a module whose *production* code still trips Miri, with the
  # finding that owns it. They are a work queue, not a settled state — deleting a
  # line here is part of fixing the thing it names, and no skip may be added
  # without a finding.
  #
  #   wels_thread_pool  F12  every worker takes `&mut` to the one shared pool
  #                          (data race on the retag; Phase 7 owns it)
  #
  # `encoder_ext` was here for F13's last production site — `InitDqLayers` holding
  # `&mut` into `sSpatialLayers` across an aliasing use. Phase 6 session A's
  # encoder probe reproduced it exactly and closed it as a 20-derivation family
  # (T6.A1); session B then ran the only two tests the filter matched
  # (`request_memory_svc_builds_the_parameter_sets`, `..._the_dq_layers`) under
  # this step's flags — 2 passed / 0 failed, Miri clock 16.92s — and deleted the
  # line, S15's "deleting the line is part of fixing what it names".
  #
  # `svc_mode_decision` was here for F13's fourth site — `SWelsFuncPtrList` being
  # self-referential, `pfMdCost` pointing into `pfSampleSad`/`pfSampleSatd` in the
  # same struct. Phase 4a de-virtualized those two slots into `CostFamily` tags,
  # so there is no interior pointer left for a `&mut` reborrow to invalidate, and
  # the skip is deleted rather than carried.
  #
  # `manage_dec_ref` was here for F13's first site, the `as_ptr()`/`as_mut_ptr()`
  # list shifts. Phase 5 T5.B2 replaced all six with `copy_within` and repaired
  # the three tests that took a second `&mut` to a picture the list already held
  # — which is what the skip had been hiding, and is F18's lesson again: the
  # backlog behind a skip is not confined to the code the skip was written for.
  MIRI_SKIPS=(--skip wels_thread_pool)
  hdr "miri (--lib, minus the F12 skip)"
  if ! rustup toolchain list 2>/dev/null | grep -q nightly; then
    skip "miri: no nightly toolchain (rustup toolchain install nightly)"
  else
    # -Zmiri-ignore-leaks: this gate is for *undefined behaviour*, not for leaks. The
    # port allocates C-style through `WelsMalloc` on purpose and frees through paired
    # destructors the unit tests mostly do not call, so leak-checking a mid-refactor
    # transliteration reports the design rather than a defect. Phase 8 (the API layer)
    # is where ownership becomes Rust's and the leak check becomes meaningful.
    #
    # -Zmiri-disable-isolation: the encode-path probe
    # (`encode_loop_runs_over_a_macroblock_grid_under_the_aliasing_checker`, live
    # since Phase 6 session B) drives `WelsEncoderEncodeExt`, and `EncodeFrameInternal`
    # calls `WelsTime()` — `SystemTime::now()`, the library's one clock site
    # (`wels_encoder_ext.rs`) — around every frame. Under isolation that call is an
    # error, so the probe cannot run without the flag. It disables host isolation
    # and nothing else: aliasing (Stacked Borrows) and validity checking are exactly
    # what they were. The forbidden list stands — `-Zmiri-disable-stacked-borrows`,
    # `-Zmiri-disable-validation`, and anything else that weakens the checker.
    run_miri lib "--lib (whole library, minus the F12 skip)" \
      "-Zmiri-ignore-leaks -Zmiri-disable-isolation" --lib -- "${MIRI_SKIPS[@]}"
  fi
fi

[ "$LEVEL" = full ] && LEVEL_DONE=1

# ---------------------------------------------------------------------------
# 6. Phase-exit Miri: the differential tests, which drive the raw-pointer
#    reference implementations as well as the safe ones.
#
# The file list is discovered, so an empty list is a step that reports nothing
# at all — the same class of hole as F17 one level up, and it gets a loud FAIL
# rather than a silently empty loop. These files retire as the raw code they
# compare against is deleted (T3.1b deleted the reader half of
# safe_bits_differential); the day the last one goes, this gate's contract
# changes and that should be a deliberate edit here, not a quiet no-op.
# No -Zmiri-ignore-leaks here, matching what this step has always run.
# ---------------------------------------------------------------------------
if [ "${LEVEL_DONE:-0}" != 1 ]; then
  diff_tests=$(cd "$CRATE/tests" && ls *differential*.rs 2>/dev/null | sed 's/\.rs$//')
  if [ -z "$diff_tests" ]; then
    hdr "miri (differential integration tests)"
    fail "miri differential: no tests/*differential*.rs found — the phase-exit Miri gate ran nothing"
  fi
  for t in $diff_tests; do
    hdr "miri (--test $t)"
    run_miri "$t" "--test $t" "" --test "$t"
  done
fi

# ---------------------------------------------------------------------------
# 7. Fuzz (plan §7.2 gate 6) — T7 is deferred by direction; there is no
#    corpus-replay net. Said out loud every run so the absence stays visible.
# ---------------------------------------------------------------------------
skip "fuzz corpus replay: the fuzz crate (Phase 0 T7) was never built — no corpus net"

# ---------------------------------------------------------------------------
# Pipeline audit (2026-08-10, F17's commit). Every `if`/verdict in this script,
# and where its status comes from:
#
#   cargo build --all-targets  PIPESTATUS[0]; the display grep may match
#                        nothing on a fully cached build, so unlike the benches
#                        its emptiness is not a signal and is not checked  sound
#   run_cargo_test       PIPESTATUS[0] + parsed totals + the ignored set   sound
#   unsafe_ratchet.sh    direct exit status, no pipeline                   sound
#   diffharness build.sh redirect, not a pipe                              sound
#   sweep_gate           PIPESTATUS[0] + tally corroboration (fixed here)  sound
#   decode / encode bench PIPESTATUS[0] + [2] + MISMATCH/DIFFER (fixed)    sound
#   run_miri (both steps) PIPESTATUS[0] + totals + zero-test (fixed here)  sound
#   `rustup toolchain list | grep -q nightly`   grep's status IS the question
#   `command -v ffmpeg`, `grep -q` on logs      status is the question
#   `tally=$(grep … | tail -1)`                 substitution, value used, not status
#
# No `set -o pipefail`, on purpose: the two bench steps pipe into display greps
# that legitimately return 1, `sweep_gate`'s tally grep likewise, and the
# nightly probe *wants* grep's status. Per-step PIPESTATUS is the pattern this
# script already proved out in run_cargo_test; pipefail would trade one class of
# wrong verdict for another.
# ---------------------------------------------------------------------------
printf '\n%s\n' "=========================== gate battery: $LEVEL ==========================="
for r in "${RESULTS[@]}"; do printf '%s\n' "$r"; done
printf '%s\n' "---------------------------------------------------------------------------"
printf '%d passed, %d failed, %d skipped   (logs in %s)\n' "$PASS" "$FAIL" "$SKIP" "$LOGS"

# The last line is the verdict, and it matches the exit code. A session B misread
# took a wrapper's exit status for this script's; there is now one unmissable
# string to grep for and nothing else to mistake for it.
if [ "$FAIL" -eq 0 ]; then
  printf 'OVERALL: PASS\n'
  exit 0
else
  printf 'OVERALL: FAIL (%d steps failed)\n' "$FAIL"
  exit 1
fi
