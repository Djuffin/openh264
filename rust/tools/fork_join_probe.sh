#!/bin/bash
# The `fork_join_encodes` pair — run in the prescribed parallel form, with the
# S61/F140 tripwire against `fork_join_baseline.txt`.
#
#   usage: rust/tools/fork_join_probe.sh [outdir]
#
# These are the two Miri data-race probes `gates.sh session` skips by name (~58
# min as a pair). Run this at a session close, and at every landing that touches
# `svc_encode_slice.rs`, `slice_multi_threading.rs` or `rec_view.rs` — those are
# the seam files, and a worker-vs-worker race is invisible to every other gate in
# the project (F223).
#
# One compile, then two concurrent shards: the compile is a run with a filter
# nothing matches, NOT `--no-run` (which reports up-to-date without building the
# interpreted target, so the shards would then race for the build lock). Same
# pattern as `gates.sh`'s Miri lane.
#
# Do NOT `pkill -f` on a pattern that matches this script or its children — a
# previous session killed its own healthy run that way. Stop it by job/PID.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
CRATE=$(cd "$HERE/../crates/openh264-rs" && pwd)
OUT=${1:-$CRATE/target/fork-join-probe}
BASE="$HERE/fork_join_baseline.txt"
FLAGS="-Zmiri-ignore-leaks -Zmiri-disable-isolation"
mkdir -p "$OUT"

t0=$(date +%s)
echo "=== compile (zero-match filter; warm target/miri makes this seconds)"
(cd "$CRATE" && MIRIFLAGS="$FLAGS" cargo +nightly miri test --lib -- \
   __fork_join_probe_compile_only__) > "$OUT/compile.log" 2>&1
rc=$?
echo "$rc" > "$OUT/compile.rc"
if [ "$rc" -ne 0 ]; then
  tail -20 "$OUT/compile.log"
  echo "FAIL: the compile step failed (rc=$rc) — see $OUT/compile.log"
  exit 1
fi
echo "    compile ok in $(( $(date +%s) - t0 ))s"

t1=$(date +%s)
echo "=== the pair, concurrently"
(cd "$CRATE" && MIRIFLAGS="$FLAGS" cargo +nightly miri test --lib -- \
   fork_join_encodes_a_multi_slice_frame) > "$OUT/probe_a.log" 2>&1 &
pa=$!
(cd "$CRATE" && MIRIFLAGS="$FLAGS" cargo +nightly miri test --lib -- \
   fork_join_encodes_a_frame_whose_slice_boundary) > "$OUT/probe_b.log" 2>&1 &
pb=$!
wait $pa; rca=$?
wait $pb; rcb=$?
pair=$(( $(date +%s) - t1 ))

# F17: corroborate each rc against its parsed totals, and fail loudly on a probe
# that ran zero tests — a renamed probe must not pass by vanishing.
parse() { grep -E '^test result' "$1" | tail -1; }
secs() { parse "$1" | grep -oE 'finished in [0-9.]+s' | grep -oE '[0-9.]+'; }
passed() { parse "$1" | grep -oE '[0-9]+ passed' | grep -oE '[0-9]+'; }
sa=$(secs "$OUT/probe_a.log"); sb=$(secs "$OUT/probe_b.log")
pa_n=$(passed "$OUT/probe_a.log"); pb_n=$(passed "$OUT/probe_b.log")

bad=0
check() {  # $1 = label, $2 = rc, $3 = passed-count, $4 = log
  if [ "$2" -ne 0 ]; then
    echo "FAIL $1: rc=$2 — see $4"; bad=1
  elif [ "${3:-0}" -ne 1 ]; then
    echo "FAIL $1: ran ${3:-0} tests, expected 1 — renamed or filtered out; see $4"; bad=1
  else
    echo "PASS $1: 1 passed / 0 failed"
  fi
}
check probe_a "$rca" "${pa_n:-0}" "$OUT/probe_a.log"
check probe_b "$rcb" "${pb_n:-0}" "$OUT/probe_b.log"
echo "    probe_a ${sa:-?}s   probe_b ${sb:-?}s   pair wall ${pair}s"

# --- the tripwire (S61/F140): warn past 1.3x, never fail.
data=$(grep -v '^#' "$BASE" 2>/dev/null | awk 'NF {print; exit}')
if [ -n "${data:-}" ]; then
  ba=$(echo "$data" | awk '{print $1}'); bb=$(echo "$data" | awk '{print $2}')
  bp=$(echo "$data" | awk '{print $3}')
  awk -v a="${sa:-0}" -v b="${sb:-0}" -v p="$pair" -v ba="$ba" -v bb="$bb" -v bp="$bp" 'BEGIN{
    ra = (ba>0)? a/ba : 0; rb = (bb>0)? b/bb : 0; rp = (bp>0)? p/bp : 0
    printf "  S61: probe_a %.2fs vs %.2fs (%.3fx), probe_b %.2fs vs %.2fs (%.3fx), pair %ds vs %ds (%.3fx)\n", a,ba,ra,b,bb,rb,p,bp,rp
    if (ra>1.3 || rb>1.3 || rp>1.3) {
      print "  *** S61 WARNING: past the 1.3x tripwire. This is a finding to file (F140), not"
      print "  *** a nuisance to route around; the close report must quote both numbers. Wall"
      print "  *** here is load-sensitive (F170) — re-measure on a quiet machine first."
    }
  }'
  echo "  (a session close then replaces the data line in $BASE)"
else
  echo "  *** S61: no baseline in $BASE — this run compared against nothing. Seed it."
fi

[ "$bad" -eq 0 ] && echo "OVERALL: PASS" || { echo "OVERALL: FAIL"; exit 1; }
