#!/bin/bash
# f3_arm.sh — one arm of the F3 ablation, at a fixed density under a fixed load.
#
#   usage: bash rust/tools/f3_arm.sh [runs]        # default 2000
#   env:   F3_LOAD=0    do not start the compile load (you are supplying it)
#          F3_STREAMS=4 number of rustc streams the built-in load generator runs
#
# WHY THIS IS A SCRIPT AND NOT A LOOP YOU RETYPE. F3's rate is a function of two
# things the finding's own history keeps re-learning: how busy the box is, and how
# tightly the runs are packed. Measurement 88 measured 1/120 with profiles and rc
# values interleaved; measurement 89 measured 1/63 on the same box the same day,
# purely because its alternation ran four encodes back to back with no switch
# between them. An after-arm run at a different density than the before-arm does
# not answer the ablation's question, so the arm is a committed artefact with the
# density baked in, not a shape each session re-invents.
#
# The configuration is measurement 86's fastest reproducer on record:
#   mt CiscoVT2people_320x192_12fps sm=3 n=600 t=4 cabac=1
# balanced over {debug,release} x rc{0,1}, interleaved, SERIAL by construction —
# the two profiles share out/ file names, so two runs must never overlap.
# bash, not zsh: measurement 78's trap.
#
# ARMS ON RECORD
#   before  b08d6c47, untouched tree, 3000 runs, 25 hits, 1/120   (measurement 88)
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
DIFF="$HERE/diffharness"
YUV="$DIFF/out/CiscoVT2people_320x192_12fps_loop18.yuv"

RUNS=${1:-2000}
STREAMS=${F3_STREAMS:-4}
# Four encodes per iteration: {debug,release} x rc{0,1}. That product IS the
# density; changing it changes the rate, which is the thing being compared.
PER_ITER=4
ITERS=$(( (RUNS + PER_ITER - 1) / PER_ITER ))

[ -f "$YUV" ] || { echo "missing clip: $YUV" >&2; exit 2; }
for p in debug release; do
  [ -x "$DIFF/rust_enc/target/$p/rust_enc" ] || {
    echo "missing $p driver — run: RUST_ENC_PROFILE=$p bash $DIFF/build.sh" >&2; exit 2; }
done

LOAD_PIDS=()
start_load() {
  # Four INDEPENDENT rebuild loops, each on its own target dir. One stream is not
  # a saturating load — a single crate's rebuild is largely a serial frontend and
  # left the box at load average 5.7 of 8 in arm 1. Four put it at 12-15 with 6+
  # concurrent rustc, which is the load every arm on record was measured under.
  # Nothing this runs writes anything the diffharness reads or executes.
  local i
  for i in $(seq "$STREAMS"); do
    (
      export CARGO_TARGET_DIR=/tmp/loadtarget$i
      cd "$ROOT/rust/crates/openh264-rs" || exit
      while true; do touch src/lib.rs; cargo build --release -j 4 --quiet 2>/dev/null; done
    ) &
    LOAD_PIDS+=($!)
  done
  sleep 20   # let the streams reach steady state before the first encode
}
stop_load() {
  local p
  for p in "${LOAD_PIDS[@]:-}"; do [ -n "$p" ] && kill -- -"$p" 2>/dev/null; kill "$p" 2>/dev/null; done
  pkill -f 'CARGO_TARGET_DIR=/tmp/loadtarget' 2>/dev/null
  for i in $(seq "$STREAMS"); do rm -rf "/tmp/loadtarget$i" 2>/dev/null; done
}

if [ "${F3_LOAD:-1}" = 1 ]; then
  set -m               # own process group per stream, so stop_load can reach the children
  start_load
  trap stop_load EXIT INT TERM
  set +m
fi

hits=0; runs=0
t0=$(date +%s)
echo "F3 arm: $ITERS iterations x $PER_ITER = $((ITERS * PER_ITER)) runs, load=${F3_LOAD:-1} streams=$STREAMS"

# The arm loop itself, verbatim from T7.A0 (safety_refactor_log.md) — the four
# encodes per iteration and their order are the density.
for i in $(seq "$ITERS"); do
  for prof in debug release; do
    for rc in 0 1; do
      if ! RUST_ENC_PROFILE=$prof bash "$DIFF/compare.sh" \
            "$YUV" 320 192 18 26 1 -1 "$rc" 0 3 600 4 >/dev/null 2>&1; then
        hits=$((hits + 1))
        echo "  HIT $hits  iter=$i prof=$prof rc=$rc  (run $((runs + 1)))"
      fi
      runs=$((runs + 1))
    done
  done
  if [ $((i % 50)) = 0 ]; then
    echo "  ... $runs runs, $hits hits, $(( $(date +%s) - t0 ))s"
  fi
done

t1=$(date +%s)
echo "-------------------------------------------------------------"
if [ "$hits" -gt 0 ]; then
  echo "F3 arm: $hits hits in $runs runs = 1/$((runs / hits)), $((t1 - t0))s wall"
else
  echo "F3 arm: 0 hits in $runs runs, $((t1 - t0))s wall"
fi
