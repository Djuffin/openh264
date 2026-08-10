#!/bin/bash
# The gate battery — plan §7.2, one command.
#
#   usage: rust/tools/gates.sh [level]
#
#     commit   cargo test (debug + release) + the unsafe ratchet          (~2 min)
#     family   commit + diffharness sweeps st/mt/def in BOTH profiles     (~5 min)
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
# 2. The unsafe ratchet (plan §7.1).
# ---------------------------------------------------------------------------
hdr "unsafe ratchet"
if bash "$HERE/unsafe_ratchet.sh" check; then
  pass "unsafe ratchet: no per-file increase"
else
  fail "unsafe ratchet: a file x metric increased"
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
  hdr "diffharness sweep st mt def ($prof)"
  if ! RUST_ENC_PROFILE="$prof" bash "$DIFF/build.sh" > "$LOGS/build_$prof.log" 2>&1; then
    fail "sweep ($prof): harness build failed — see $LOGS/build_$prof.log"
    return
  fi
  t0=$(date +%s)
  RUST_ENC_PROFILE="$prof" bash "$DIFF/sweep.sh" st mt def 2>&1 | tee "$log" | tail -20
  rc=${PIPESTATUS[0]}
  t1=$(date +%s)
  local tally wall=$((t1 - t0))
  tally=$(grep -E '^PASS=[0-9]+ FAIL=[0-9]+' "$log" | tail -1)
  [ -z "$tally" ] && tally=$(tail -3 "$log" | tr '\n' ' ')
  printf '  %s   (%ss wall)\n' "$tally" "$wall"
  if [ "$rc" -eq 0 ]; then
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
# ---------------------------------------------------------------------------
if [ "${LEVEL_DONE:-0}" != 1 ]; then
  hdr "decode_1080p_bench"
  if (cd "$CRATE" && BENCH_ITERS="${BENCH_ITERS:-3}" cargo bench --bench decode_1080p_bench 2>&1) \
       | tee "$LOGS/decode_bench.log" | grep -E 'Rust :|C\+\+  :|ratio:|MISMATCH|frames'; then
    if grep -q 'MISMATCH' "$LOGS/decode_bench.log"; then
      fail "decode_1080p_bench: output mismatch"
    else
      pass "decode_1080p_bench: all streams bit-identical"
    fi
  else
    fail "decode_1080p_bench: non-zero exit"
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
    if (cd "$CRATE" && FFMPEG="$FFMPEG" BENCH_REQUIRE_FFMPEG=1 \
          cargo bench --bench c_vs_rust_bench 2>&1) \
         | tee "$LOGS/encode_bench.log" | grep -E 'ms|IDENTICAL|DIFFER'; then
      if grep -q 'DIFFER' "$LOGS/encode_bench.log"; then
        fail "c_vs_rust_bench: a row differed"
      else
        pass "c_vs_rust_bench: all rows bit-identical"
      fi
    else
      fail "c_vs_rust_bench: non-zero exit"
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
  #   manage_dec_ref    F13  `AddLongTermToList` copies through an `as_ptr()` the
  #                          following `as_mut_ptr()` has already invalidated
  #   encoder_ext       F13  `InitDqLayers` holds `&mut` into `sSpatialLayers`
  #                          across an aliasing use
  #
  # `svc_mode_decision` was here for F13's fourth site — `SWelsFuncPtrList` being
  # self-referential, `pfMdCost` pointing into `pfSampleSad`/`pfSampleSatd` in the
  # same struct. Phase 4a de-virtualized those two slots into `CostFamily` tags,
  # so there is no interior pointer left for a `&mut` reborrow to invalidate, and
  # the skip is deleted rather than carried.
  MIRI_SKIPS=(--skip wels_thread_pool --skip manage_dec_ref --skip encoder_ext)
  hdr "miri (--lib, minus the F12/F13 skips)"
  if ! rustup toolchain list 2>/dev/null | grep -q nightly; then
    skip "miri: no nightly toolchain (rustup toolchain install nightly)"
  # -Zmiri-ignore-leaks: this gate is for *undefined behaviour*, not for leaks. The
  # port allocates C-style through `WelsMalloc` on purpose and frees through paired
  # destructors the unit tests mostly do not call, so leak-checking a mid-refactor
  # transliteration reports the design rather than a defect. Phase 8 (the API layer)
  # is where ownership becomes Rust's and the leak check becomes meaningful.
  elif (cd "$CRATE" && MIRIFLAGS="${MIRIFLAGS:-} -Zmiri-ignore-leaks" \
          cargo +nightly miri test --lib -- "${MIRI_SKIPS[@]}" 2>&1) \
         | tee "$LOGS/miri_lib.log" | tail -5; then
    pass "miri --lib (whole library, minus the F12/F13 skips)"
  else
    fail "miri --lib — see $LOGS/miri_lib.log"
  fi
fi

[ "$LEVEL" = full ] && LEVEL_DONE=1

# ---------------------------------------------------------------------------
# 6. Phase-exit Miri: the differential tests, which drive the raw-pointer
#    reference implementations as well as the safe ones.
# ---------------------------------------------------------------------------
if [ "${LEVEL_DONE:-0}" != 1 ]; then
  for t in $(cd "$CRATE/tests" && ls *differential*.rs 2>/dev/null | sed 's/\.rs$//'); do
    hdr "miri (--test $t)"
    if (cd "$CRATE" && cargo +nightly miri test --test "$t" 2>&1) \
         | tee "$LOGS/miri_$t.log" | tail -5; then
      pass "miri --test $t"
    else
      fail "miri --test $t — see $LOGS/miri_$t.log"
    fi
  done
fi

# ---------------------------------------------------------------------------
# 7. Fuzz (plan §7.2 gate 6) — T7 is deferred by direction; there is no
#    corpus-replay net. Said out loud every run so the absence stays visible.
# ---------------------------------------------------------------------------
skip "fuzz corpus replay: the fuzz crate (Phase 0 T7) was never built — no corpus net"

# ---------------------------------------------------------------------------
printf '\n%s\n' "=========================== gate battery: $LEVEL ==========================="
for r in "${RESULTS[@]}"; do printf '%s\n' "$r"; done
printf '%s\n' "---------------------------------------------------------------------------"
printf '%d passed, %d failed, %d skipped   (logs in %s)\n' "$PASS" "$FAIL" "$SKIP" "$LOGS"
[ "$FAIL" -eq 0 ]
