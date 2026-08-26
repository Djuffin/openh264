#!/bin/bash
# The gate battery — plan §7.2, one command.
#
#   usage: rust/tools/gates.sh [level]
#
#     commit   build --all-targets + cargo test (debug + release) + ratchet (~2 min)
#     family   commit + diffharness sweeps st/mt/def/sl in BOTH profiles  (~5 min)
#     session  family + Miri --lib, NO benches — the Phase 9 session close
#              (D-gate-4, 2026-08-22; re-architected under D-gate-6, 2026-08-24,
#              which capped the whole level — at 15 minutes as first ruled, and
#              at 20 MINUTES (1200 s) as amended 2026-08-26).
#                  bash rust/tools/gates.sh session
#              The Miri step runs as a parallel background lane (one compile +
#              five concurrent shards, encoder-scoped by construction — no
#              MIRI_SCOPE needed at this level, and the encode probes drive
#              small geometries; see the D-gate-6 block below for what this
#              level no longer covers and where it is restored). The benches
#              stay at the phase close (D-gate-1).
#     full     family + decode bench + encoder bench + Miri --lib         (default)
#     exit     full + Miri over the differential integration tests
#              + the three C-ABI boundary gates: the cdylib's export list, the
#                external dlopen harness (T8.C3/T8.C5), and upstream's own
#                `test/api` gtest suite against the cdylib, ratcheted against
#                `abi_harness/gtest_known_failures.txt` (T8b.A1)       (phase exits)
#
#   env:
#     FFMPEG=/path/to/ffmpeg   required for the encoder bench; without it that
#                              gate SKIPs loudly (the fallback measures the
#                              frame-skip path, not encoding — perf_baseline.md §2)
#     BENCH_ITERS=3            passes per bench row
#     SKIP_SWEEP=1             drop the sweep gates (use only when you know why)
#     MIRI_SCOPE=encoder       run the Miri `--lib` step over the encoder side only:
#                              the skip list gains `--skip decoder::`, which is exactly
#                              the 101 decoder-module tests including the three decoder
#                              probes (the expensive ones — session A measured them at
#                              665s of the 835s step). Phase 6 decision D-gate-2: a
#                              session that touches only `src/encoder`/`src/processing`
#                              buys nothing from re-running them, and the phase runs the
#                              full unscoped step at its own exit. Unset (or any other
#                              value) = the whole library, which is the default and what
#                              every phase exit must use. Phase 9 (D-gate-4) pairs it with
#                              the `session` level at every session close: F114 was a
#                              retag the byte gates passed 535/535 and only Miri saw.
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
  commit|family|session|full|exit) ;;
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
# D-gate-6 (the user, 2026-08-24): the WHOLE session gate is capped at 15
# minutes — "even if we need to reduce the amount of tests that we run".
#
# **AMENDED 2026-08-26 (the user, from F185): the cap is 20 minutes, 1200 s.**
# X2's battery measured 977 s with full coverage; the amendment's reasoning is
# that a round number bought by a named coverage cut is worse than 977 s that
# covers everything, and that the Miri lane's parallelism taxing the native lane
# is a trade this block predicted without pricing. Until F170's CPU-time fix
# lands at J, the wall number carries F170's caveat about machine load. This
# header read "15 minutes" for two sessions after the amendment; H2 corrected it.
#
# The ruling landed after a session-level run was stopped at ~40 minutes; D-gate-5's
# probe shrink alone could not meet it, because the serial battery already
# spends ~4 min in builds/tests and ~3-4 min in sweeps before Miri starts, and
# the Miri step's own compile is minutes. What meets it is parallelism the
# machine already has:
#
#   native lane (this shell):  build, tests, ratchet, census, both sweeps
#   miri lane   (background):  one compile, then FOUR concurrent
#                              `cargo miri test` shards — the CAVLC and
#                              size-limited encode probes are one shard each
#                              (the largest single tests, F140/F141),
#                              everything else splits into an encoder shard
#                              and an api/common/safe shard
#
# The lanes do not contend: cargo-miri builds under target/miri with its own
# lock, the diffharness builds in its own tree, and each Miri interpreter is
# single-threaded, so five shards cost five cores beside the native builds.
#
# What the session level no longer covers (named per D-cov-1; every item is
# restored at `full`/`exit`, which export MIRI_FULL=1 and run the serial
# un-sharded step):
#   - the size-limited probe drives 48x32x2 under Miri, so session-scope Miri
#     does not see the slice-buffer realloc chain (the probe's own doc says
#     why 112x96 cannot be afforded there);
#   - D-gate-7 (the user, 2026-08-24): the CABAC/low-complexity grid probe is
#     `#[cfg_attr(miri, ignore)]` — its Miri axes are covered more deeply by
#     the size-limited probe (CABAC + stash/restore, LOW_COMPLEXITY) and the
#     fork probes; it runs natively on every cargo test. The same ruling asks
#     that encoder Miri probes run as SEPARATE invocations — the lane already
#     does (one shard per probe), and full/exit run each probe as its own
#     step below instead of one monolithic --lib pass;
#   - the two fork/join probes are skipped BY NAME in the encoder shard: live
#     (un-ignored) since T9.E8, their first green runs measured 3356 s and
#     3449 s (~57 min as a parallel pair) — MIN_NUM_MB_PER_SLICE floors their
#     geometry at 49 macroblocks and two frames is the inter-coverage floor,
#     so no shrink exists and no 15-minute gate can carry them. They run at
#     full/exit, and any session that touches the fork, the slice structures,
#     or deblocking runs them explicitly once at its close (the two probes in
#     parallel halve the wall; -Zmiri-report-progress makes the hour visible):
#       MIRIFLAGS='-Zmiri-ignore-leaks -Zmiri-disable-isolation -Zmiri-report-progress' \
#         cargo +nightly miri test --lib -- fork_join_encodes
#
# Verdicts follow F17: each shard's rc is captured via `wait`, corroborated
# against its parsed totals, and a shard that ran zero tests fails loudly — a
# renamed probe must not pass by vanishing (this script's own header rule).
# ---------------------------------------------------------------------------
MIRI_SHARDS=(cavlc sizelimited enc other)
MIRI_LANE_FLAGS="-Zmiri-ignore-leaks -Zmiri-disable-isolation"
miri_session_lane() {
  local t0 rc
  t0=$(date +%s)
  # The compile step is a run with a filter nothing matches, NOT `--no-run`:
  # measured 2026-08-24, `cargo miri test --no-run` reports up-to-date without
  # building the interpreted target, so the four shards would then race for the
  # build lock and one of them pays the whole compile while three block. A
  # zero-match run compiles everything and interprets nothing; its verdict is
  # the rc alone (zero tests is this step's expected shape, unlike the shards').
  (cd "$CRATE" && MIRIFLAGS="${MIRIFLAGS:-} $MIRI_LANE_FLAGS" \
     cargo +nightly miri test --lib -- __miri_lane_compile_only__) > "$LOGS/miri_compile.log" 2>&1
  rc=$?
  echo "$rc" > "$LOGS/miri_compile.rc"
  if [ "$rc" -ne 0 ]; then
    echo $(( $(date +%s) - t0 )) > "$LOGS/miri_lane_wall"
    return
  fi
  # The two catch-all shards split the ~280 non-probe tests: `enc` is the
  # encoder module minus the probe shards' tests and the fork probes (see the
  # header), `other` is everything outside encoder:: and decoder:: (api,
  # common, safe, bits). The un-sharded rest measured 560 s against the probe
  # shards' 188-237, so it was the critical path; halved, no shard dominates.
  # decoder:: is out of `other` unconditionally at this level — the session
  # level *is* the encoder-scoped close (D-gate-4 pairs them), and a session
  # that touches decoder code runs the serial `full` step for it instead.
  local pids=()
  (cd "$CRATE" && MIRIFLAGS="${MIRIFLAGS:-} $MIRI_LANE_FLAGS" cargo +nightly miri test --lib -- \
     encode_loop_runs_with_cavlc) > "$LOGS/miri_shard_cavlc.log" 2>&1 &
  pids+=($!)
  (cd "$CRATE" && MIRIFLAGS="${MIRIFLAGS:-} $MIRI_LANE_FLAGS" cargo +nightly miri test --lib -- \
     encode_loop_runs_over_size_limited) > "$LOGS/miri_shard_sizelimited.log" 2>&1 &
  pids+=($!)
  (cd "$CRATE" && MIRIFLAGS="${MIRIFLAGS:-} $MIRI_LANE_FLAGS" cargo +nightly miri test --lib -- \
     'encoder::' --skip 'encode_loop_runs' --skip 'fork_join_encodes') \
     > "$LOGS/miri_shard_enc.log" 2>&1 &
  pids+=($!)
  (cd "$CRATE" && MIRIFLAGS="${MIRIFLAGS:-} $MIRI_LANE_FLAGS" cargo +nightly miri test --lib -- \
     --skip 'encoder::' --skip 'decoder::') > "$LOGS/miri_shard_other.log" 2>&1 &
  pids+=($!)
  local i=0
  local pid
  for pid in "${pids[@]}"; do
    wait "$pid"
    echo $? > "$LOGS/miri_shard_${MIRI_SHARDS[$i]}.rc"
    i=$((i+1))
  done
  echo $(( $(date +%s) - t0 )) > "$LOGS/miri_lane_wall"
}

# S61 (F140): the gate's own cost is a gated quantity. Prints this run's Miri
# wall beside the previous session close's (miri_wall_baseline.txt) and WARNs
# past 1.3x — warn, never fail: machine variance is real, and the duty S61
# assigns is that the close *report* quotes both numbers and files a finding.
s61_report() {  # $1 = this run's miri wall seconds
  local wall=$1 prev ratio
  local base="$HERE/miri_wall_baseline.txt"
  prev=$(grep -v '^#' "$base" 2>/dev/null | awk 'NF {print $1; exit}')
  if [ -n "${prev:-}" ] && [ "$prev" -gt 0 ] 2>/dev/null; then
    ratio=$(awk -v a="$wall" -v b="$prev" 'BEGIN { printf "%.2f", a / b }')
    printf '  S61: miri wall %ss against the previous close'\''s %ss — ratio %s\n' \
      "$wall" "$prev" "$ratio"
    if awk -v a="$wall" -v b="$prev" 'BEGIN { exit !(a > 1.3 * b) }'; then
      printf '  *** S61 WARNING: the Miri step is %sx the previous session close — past the\n' "$ratio"
      printf '  *** 1.3x tripwire. This is a finding to file (F140'\''s rule), not a nuisance to\n'
      printf '  *** route around; the close report must quote both numbers.\n'
    fi
    printf '  (a session close then updates %s)\n' "$base"
  else
    # The baseline is part of the instrument (S58: a silent gap reads as a
    # clean run). Missing or unparsable means the tripwire measured nothing.
    printf '  *** S61: no baseline in %s — this run measured %ss and compared it\n' "$base" "$wall"
    printf '  *** against nothing. Seed the file with that number.\n'
  fi
}

MIRI_LANE_PID=""
if [ "$LEVEL" = session ] && rustup toolchain list 2>/dev/null | grep -q nightly; then
  rm -f "$LOGS/miri_compile.rc" "$LOGS/miri_lane_wall"
  for s in "${MIRI_SHARDS[@]}"; do rm -f "$LOGS/miri_shard_$s.rc"; done
  miri_session_lane &
  MIRI_LANE_PID=$!
  printf 'miri lane launched in parallel (pid %s; logs %s/miri_shard_*.log)\n' \
    "$MIRI_LANE_PID" "$LOGS"
fi

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
  hdr "diffharness sweep st mt def sl ltr ps dl bg ($prof)"
  if ! RUST_ENC_PROFILE="$prof" bash "$DIFF/build.sh" > "$LOGS/build_$prof.log" 2>&1; then
    fail "sweep ($prof): harness build failed — see $LOGS/build_$prof.log"
    return
  fi
  t0=$(date +%s)
  # **`ps` and `dl` joined the exit list in Phase 8b (T8b.B3 and T8b.C2).** Both
  # cover axes the port *refused at `InitializeExt`* until this phase, so neither
  # had a byte of coverage and neither could have been in this list before:
  # `ps` is the five `eSpsPpsIdStrategy` values, `dl` is `iSpatialLayerNum` 2/3/4 x
  # denoise on/off — the only preset that runs `METHOD_DOWNSAMPLE` or
  # `METHOD_DENOISE` at all. 369 -> 535 configurations per profile.
  #
  # **`bg` joined in Phase 9 session B4 (D-ref-1), for the same reason.**
  # `bEnableBackgroundDetection` was pinned `false` in both drivers, so the whole
  # background family — `WelsMdBackgroundMbEnc`, `VaaBackgroundMbDataUpdate`,
  # `WelsMdUpdateBGDInfo`, the analyzer's `BackgroundDetection` — had never been
  # compared against the reference on a single byte, while `FillDefault` leaves the
  # flag ON for every ordinary application. 535 -> 583 configurations per profile.
  #
  # The "505" this comment carried from Phase 8b until B4 was wrong and had been
  # wrong since `dl` landed: the list has measured **535** at every commit since,
  # which is the number the findings file quotes throughout. Corrected here by
  # running the list and reading the tally rather than by re-adding the presets.
  RUST_ENC_PROFILE="$prof" bash "$DIFF/sweep.sh" st mt def sl ltr ps dl bg 2>&1 | tee "$log" | tail -20
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
# 3b. The gtest simulcast smoke (D-fid-3, the user, 2026-08-26).
#
# **Why this exists.** `gtest_stretch.sh --check` in full runs only at `exit`
# (6c below), and between session E3 and session X2 it did not run at all: the
# binary took `Abort trap: 6` inside `EncodeDecodeTestAPI.SimulcastAVC_SPS_PPS_LISTING`,
# printed no tally, and every session in between closed green at `family` with a
# red `exit` gate in the tree nobody was running (F173). A gate that only the
# phase close executes is a gate that discovers a five-session-old regression at
# the phase close.
#
# So a filtered subset joins the session battery: the four `Simulcast*` rows, the
# family the abort lived in, run through the same `--check` ratchet as 6c. With a
# `--gtest_filter` the script suppresses only its "listed but never ran" arm (the
# filter is why they did not run); "failing but unlisted" and "listed but passing"
# both still apply, so this is a real red/green and not a smoke that only checks
# for a pulse.
#
# **Budget: <=60 s.** Measured 2026-08-26 on the session-X2 tree — 4.9 s of gtest
# for the four rows, ~10 s of link, and the `cargo build --release` the script
# opens with is a cache hit because `cargo test (release)` above already built it.
# The wall is printed every run so the budget stays a measured quantity.
#
# **Native lane, deliberately** (S61): this runs in the main shell beside the
# builds and sweeps, never inside the Miri lane, so the session's Miri wall stays
# comparable to the previous close's number lane-for-lane.
#
# Not at `commit`/`family` (a link plus a run per commit is not worth it) and not
# at `exit`, where 6c runs the whole suite and this would be a strict subset of
# it. rc 2 is the script's "prerequisites missing" code and SKIPs loudly, exactly
# as 6c does.
# ---------------------------------------------------------------------------
GTEST_SMOKE_FILTER='EncodeDecodeTestAPI.Simulcast*'
GTEST_SMOKE_BUDGET=60
if [ "${LEVEL_DONE:-0}" != 1 ] && [ "$LEVEL" != exit ]; then
  if [ ! -f "$ROOT/libgtest.a" ] || [ ! -f "$ROOT/libopenh264.a" ]; then
    skip "gtest simulcast smoke: prerequisites missing — run 'make -j8 libraries binaries' at the repo root"
  else
    hdr "gtest simulcast smoke ($GTEST_SMOKE_FILTER, allowlist ratchet)"
    t0=$(date +%s)
    bash "$HERE/abi_harness/gtest_stretch.sh" --check "--filter=$GTEST_SMOKE_FILTER" 2>&1       | tee "$LOGS/gtest_smoke.log"       | grep -vE '^(warning|note|help|error)|^ +[0-9]* *\||^ +\||^ +[-^=]' | tail -12
    rc=${PIPESTATUS[0]}
    smoke_wall=$(( $(date +%s) - t0 ))
    tally=$(grep -E '^gtest: ' "$LOGS/gtest_smoke.log" | tail -1 | sed 's/^gtest: //')
    printf '  %ss wall (budget %ss)
' "$smoke_wall" "$GTEST_SMOKE_BUDGET"
    if [ "$rc" -eq 2 ]; then
      skip "gtest simulcast smoke: prerequisites missing — see $LOGS/gtest_smoke.log"
    elif [ "$rc" -ne 0 ]; then
      fail "gtest simulcast smoke: a failure is unlisted, or a listed row passed (rc=$rc) — see $LOGS/gtest_smoke.log"
    elif [ -z "$tally" ]; then
      # The F173 shape itself: an abort prints no verdict line, and exit status
      # alone would not have caught it here either.
      fail "gtest simulcast smoke: exit 0 but no 'gtest: n/n' line — the binary died before its verdict; see $LOGS/gtest_smoke.log"
    elif [ "$smoke_wall" -gt "$GTEST_SMOKE_BUDGET" ]; then
      fail "gtest simulcast smoke: $tally, but ${smoke_wall}s exceeds the ${GTEST_SMOKE_BUDGET}s budget — trim the filter or re-argue the budget"
    else
      pass "gtest simulcast smoke: $tally, ${smoke_wall}s"
    fi
  fi
fi

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
 if [ "$LEVEL" = session ]; then
  # D-gate-4: the session close runs Miri but not the benches — D-gate-1 keeps every
  # perf measurement at the phase close, and the benches' bit-identity half is
  # already covered by the sweeps above. Said out loud in the summary, not omitted.
  skip "benches: not run at the session level (D-gate-4; the phase close runs them)"
 else
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
 fi  # session-level bench skip (D-gate-4)

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
  # **THE LIST IS EMPTY, and has been since T7.B4.** `wels_thread_pool` was the last
  # one — F12, "every worker takes `&mut` to the one shared pool (data race on the
  # retag)". It is deleted rather than fixed: `common/wels_thread_pool.rs` is gone and
  # the encoder forks with `std::thread::scope`, so there is no pool for a worker to
  # alias. S15's rule is honoured in the strongest available form — the line went in
  # the same commit as the code it named.
  #
  # And it is not a vacuous deletion. Nothing in the library drove
  # `iMultipleThreadIdc > 1` under Miri before, so an empty skip list would have meant
  # only that the untested path was no longer named; the same commit adds
  # `fork_join_encodes_a_multi_slice_frame_under_the_aliasing_checker`
  # (`encoder/svc_encode_slice.rs`), which drives the fork/join itself.
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
  # **F75 (T7.C10): this array is empty now, and that broke the step that matters
  # most.** macOS ships bash 3.2, where `"${arr[@]}"` on an *empty* array is an
  # unbound-variable error under `set -u` — which this script sets, deliberately. Every
  # phase exit before this one ran with at least one entry here, so the expansion was
  # never empty and the bug never fired. **T7.B4 deleted the last skip** (F12's
  # `--skip wels_thread_pool`, with the module it named), and the very next unscoped
  # run — this phase's exit — died on line 403 instead of running Miri.
  #
  # So the gate's own success condition disabled the gate. That is the third sighting
  # of this project's recurring shape (F17's `tail` exit status, F68's thread axis,
  # F74's parser): **an instrument whose empty case is untested**. Fixed at the
  # expansion below with the bash 3.2 idiom rather than by keeping a dummy entry here,
  # because an empty skip list is the correct state and should stay expressible.
  MIRI_SKIPS=()
  # SCOPE (Phase 6 session H, decision D-gate-2). `MIRI_SCOPE=encoder` adds one
  # more skip — and it is a *scope*, not a skip in the sense above: it names no
  # finding and hides no defect, because the code it drops out is code the session
  # did not touch. `--skip decoder::` is a substring filter and it was measured to
  # match the 101 `decoder::` tests and nothing else (no test outside that module
  # path carries the string). All three decoder probes are inside it, which is the
  # point: they are the expensive tests. The four encoder probes are
  # `encoder::svc_encode_slice::tests::*` and are unaffected — the check that this
  # knob is honest is that they still appear by name in the run's test list.
  #
  # Written for Phase 6's encoder sessions; Phase 9 runs it at every session close
  # through the `session` level (D-gate-4). Every phase exit runs unscoped.
  if [ "${MIRI_SCOPE:-}" = encoder ]; then
    MIRI_SKIPS+=(--skip 'decoder::')
    MIRI_DESC="--lib, encoder scope (minus decoder::)"
  else
    MIRI_DESC="--lib (whole library, no skips)"
  fi
  if [ "$LEVEL" = session ]; then
    # D-gate-6: the parallel lane launched at the top of the script; this is
    # the join. See the lane's header comment for what session scope covers.
    hdr "miri (parallel lane join: 1 compile + 4 shards — D-gate-6)"
    if [ -z "$MIRI_LANE_PID" ]; then
      skip "miri: no nightly toolchain (rustup toolchain install nightly)"
    else
      wait "$MIRI_LANE_PID"
      if [ "$(cat "$LOGS/miri_compile.rc" 2>/dev/null)" != 0 ]; then
        tail -8 "$LOGS/miri_compile.log"
        fail "miri lane: the --no-run compile failed — see $LOGS/miri_compile.log"
      else
        miri_total_passed=0
        miri_shards_bad=0
        for s in "${MIRI_SHARDS[@]}"; do
          shard_rc=$(cat "$LOGS/miri_shard_$s.rc" 2>/dev/null)
          shard_log="$LOGS/miri_shard_$s.log"
          shard_passed=$(awk '/^test result:/ { for (i=1;i<=NF;i++) if ($i=="passed;") t+=$(i-1) } END { print t+0 }' "$shard_log")
          shard_failed=$(awk '/^test result:/ { for (i=1;i<=NF;i++) if ($i=="failed;") t+=$(i-1) } END { print t+0 }' "$shard_log")
          printf '  shard %-12s %s passed / %s failed  (rc=%s)\n' "$s:" "$shard_passed" "$shard_failed" "${shard_rc:-?}"
          if [ "${shard_rc:-1}" -ne 0 ] || [ "$shard_failed" -ne 0 ]; then
            tail -12 "$shard_log"
            fail "miri shard $s: $shard_passed passed / $shard_failed failed, rc=${shard_rc:-?} — see $shard_log"
            miri_shards_bad=1
          elif [ "$shard_passed" -eq 0 ]; then
            fail "miri shard $s: ran 0 tests — a renamed probe or a dead filter; see $shard_log"
            miri_shards_bad=1
          fi
          miri_total_passed=$((miri_total_passed + shard_passed))
        done
        if [ "$miri_shards_bad" -eq 0 ]; then
          pass "miri (4 shards, session scope): $miri_total_passed passed / 0 failed"
        fi
        s61_report "$(cat "$LOGS/miri_lane_wall" 2>/dev/null || echo 0)"
      fi
    fi
  else
  hdr "miri ($MIRI_DESC)"
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
    # S61 (F140): the gate's own cost is a gated quantity. A 2.6x Miri regression
    # passed eight commits because nothing watches the instrument — the charter was
    # quoting the wall-time series (894 / 978 / 1012 s) in prose and checking it
    # nowhere. So this step times itself and compares against the previous session
    # close's number, committed in miri_wall_baseline.txt. WARN past 1.3x, never
    # fail: machine variance is real, and the duty S61 assigns is that the close
    # *report* quotes both numbers and files a finding — not that a loaded machine
    # can veto a session.
    MIRI_T0=$(date +%s)
    # D-gate-7 (the user, 2026-08-24): each encoder probe runs as its OWN
    # invocation with its own verdict line, never inside one monolithic pass —
    # so the big --lib step skips them all, and the per-probe steps follow.
    # MIRI_FULL=1 restores the probes' full drives at these un-capped tiers
    # (`full`/`exit`) — see `miri_scaled` in svc_encode_slice.rs (D-gate-6):
    # the session lane runs them small, these levels run them whole. The two
    # fork probes measured 3356 s and 3449 s green (T9.E8); running them here
    # is what "the phase exit runs the fork" means, and the wall is theirs.
    run_miri lib "$MIRI_DESC (probes split out, D-gate-7)" \
      "-Zmiri-ignore-leaks -Zmiri-disable-isolation" --lib -- \
      --skip 'encode_loop_runs' --skip 'fork_join_encodes' \
      ${MIRI_SKIPS[@]+"${MIRI_SKIPS[@]}"}   # F75: bash 3.2 + set -u, empty array
    hdr "miri (per-probe steps — D-gate-7)"
    MIRI_FULL=1 run_miri probe_cavlc "probe: cavlc+fine-MD (full drive)" \
      "-Zmiri-ignore-leaks -Zmiri-disable-isolation" --lib -- \
      encode_loop_runs_with_cavlc
    MIRI_FULL=1 run_miri probe_sizelimited "probe: size-limited dynamic slices (full drive)" \
      "-Zmiri-ignore-leaks -Zmiri-disable-isolation" --lib -- \
      encode_loop_runs_over_size_limited
    run_miri probe_fork_fixed "probe: fork/join fixed-slice (~56 min, T9.E8)" \
      "-Zmiri-ignore-leaks -Zmiri-disable-isolation -Zmiri-report-progress" --lib -- \
      fork_join_encodes_a_multi_slice_frame
    run_miri probe_fork_midrow "probe: fork/join mid-row boundary (~57 min, T9.E8)" \
      "-Zmiri-ignore-leaks -Zmiri-disable-isolation -Zmiri-report-progress" --lib -- \
      fork_join_encodes_a_frame_whose_slice_boundary
    s61_report $(( $(date +%s) - MIRI_T0 ))
  fi
  fi  # session lane join / serial step (D-gate-6)
fi

case "$LEVEL" in session|full) LEVEL_DONE=1 ;; esac

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
# 6b. The C-ABI boundary gates (plan §7.2 gate 7, Phase 8 session C; the gtest
#     ratchet joined them in Phase 8b session A, as 6c below).
#
# All of these look at the thing a consumer actually gets — the **cdylib** — and
# neither can be seen from inside the process that `cargo test` runs. The rlib the
# in-process battery links has no dynamic symbol table and no `dlopen` path, so a
# broken export list, a wrong struct size, or a thunk that only works when the caller
# was compiled by the same rustc all pass every step above.
#
# They are at `exit` rather than `commit` because each one builds a native artefact
# and the harness decodes the conformance 60 again through it (~1 min together), and
# because the surface they guard — the ABI — is exactly the surface a phase exit is
# for. `abi_exports.sh` alone is cheap enough to run by hand at any time.
#
# D-gate-3 keeps the sweeps, Miri and the benches at the phase close; these three are
# what a *session* close runs by hand (charter §4.6), and `exit` runs them again.
# ---------------------------------------------------------------------------
# F17's rule applies to both: the verdict is `${PIPESTATUS[0]}`, never the `if`'s,
# and it is corroborated by a summary line that only a completed run prints.
if [ "${LEVEL_DONE:-0}" != 1 ]; then
  hdr "abi exports (cdylib == upstream's 7)"
  bash "$HERE/abi_exports.sh" release 2>&1 | tee "$LOGS/abi_exports.log" | tail -6
  rc=${PIPESTATUS[0]}
  tally=$(grep -E '^exports: ' "$LOGS/abi_exports.log" | tail -1)
  if [ "$rc" -ne 0 ]; then
    fail "abi exports: the cdylib's export list is not exactly the seven (rc=$rc) — see $LOGS/abi_exports.log"
  elif [ -z "$tally" ]; then
    fail "abi exports: exit 0 but no 'exports: n/n' line — the script died before its verdict; see $LOGS/abi_exports.log"
  else
    pass "abi exports: $tally"
  fi

  hdr "external-ABI harness (dlopen the cdylib)"
  bash "$HERE/abi_harness/run.sh" 2>&1 | tee "$LOGS/abi_harness.log" | tail -14
  rc=${PIPESTATUS[0]}
  tally=$(grep -E '^TALLY ' "$LOGS/abi_harness.log" | tail -1 | sed 's/^TALLY //')
  if [ "$rc" -ne 0 ]; then
    fail "abi harness: a part failed (rc=$rc) — see $LOGS/abi_harness.log"
  elif [ -z "$tally" ]; then
    fail "abi harness: exit 0 but no TALLY line — the harness died before finishing; see $LOGS/abi_harness.log"
  else
    pass "abi harness: $tally"
  fi

  # -------------------------------------------------------------------------
  # 6c. Upstream's own `test/api` suite against the cdylib, ratcheted against a
  # named allowlist (Phase 8b session A, T8b.A1).
  #
  # Same shape as the two above — `PIPESTATUS[0]` plus a verdict line only a
  # completed comparison prints — and here for the same reason: it looks at the
  # cdylib from outside the process, and it is ~40 s.
  #
  # rc 2 is the script's own "prerequisites missing" code (`test/api/*.o`,
  # `libgtest.a`, `libopenh264.a` — the `make -j8 libraries binaries` products).
  # That is an environment gap, not a port defect, so it SKIPs loudly with the
  # remedy rather than reporting a codec failure that was never measured; every
  # other nonzero is the ratchet firing and fails.
  # -------------------------------------------------------------------------
  hdr "gtest test/api against the cdylib (allowlist ratchet)"
  bash "$HERE/abi_harness/gtest_stretch.sh" --check 2>&1 | tee "$LOGS/gtest_check.log" | grep -vE '^(warning|note|help|error)|^ +[0-9]* *\||^ +\||^ +[-^=]' | tail -20
  rc=${PIPESTATUS[0]}
  tally=$(grep -E '^gtest: ' "$LOGS/gtest_check.log" | tail -1 | sed 's/^gtest: //')
  if [ "$rc" -eq 2 ]; then
    skip "gtest test/api: prerequisites missing — run 'make -j8 libraries binaries' at the repo root; see $LOGS/gtest_check.log"
  elif [ "$rc" -ne 0 ]; then
    fail "gtest test/api: a failure is unlisted, or a listed row passed (rc=$rc) — see $LOGS/gtest_check.log"
  elif [ -z "$tally" ]; then
    fail "gtest test/api: exit 0 but no 'gtest: n/n' line — the script died before its verdict; see $LOGS/gtest_check.log"
  else
    pass "gtest test/api: $tally"
  fi
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
#   abi_exports.sh       PIPESTATUS[0] + the `exports: n/n` line             sound
#   abi_harness/run.sh   PIPESTATUS[0] + the `TALLY` line                    sound
#   gtest_stretch.sh --check  PIPESTATUS[0] + the `gtest: n/n` line; rc 2 is
#                        the script's documented "prerequisites missing" and
#                        SKIPs rather than failing                      sound
#                        (both written as PIPESTATUS from the start; the `if
#                        pipeline; then` shape F17 names would have made each of
#                        them a reporter, and a green ABI gate that never checked
#                        anything is the worst possible one)
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
