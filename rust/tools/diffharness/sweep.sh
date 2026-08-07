#!/bin/bash
# Differential sweeps over compare.sh.
#
#   usage: rust/tools/diffharness/sweep.sh <preset> [preset ...]
#
#   presets:  st    single-threaded: 5 iRCMode x 2 init paths x 3 GOP x cabac x input
#                   plus the four slice modes                            (210 configs)
#             mt    iMultipleThreadIdc 2/4 x 4 slice modes x cabac x
#                   iRCMode x input                                      (120 configs)
#             qp    all 52 QPs x cabac x input                           (312 configs)
#             all   every preset above
#
# Exits non-zero if any configuration differs. Prints one line per failure.
#
# Written as bash on purpose. The interactive shell here is zsh, which does NOT
# word-split unquoted expansions: `for spec in "1 2"; do set -- $spec` leaves $2
# empty and every run silently gets garbage arguments. That has cost this project
# real time more than once, which is why this script exists instead of being
# rewritten from scratch each session. Keep it bash, keep the `read -r` idiom, and
# quote every expansion.
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
CMP="$HERE/compare.sh"
cd "$ROOT" || exit 1

# Per-run watchdog. macOS has no timeout(1), and a deadlocked encoder would
# otherwise stall the sweep and merely look slow.
TIMEOUT=${SWEEP_TIMEOUT:-180}

INPUTS=(
  "res/CiscoVT2people_160x96_6fps.yuv 160 96"
  "res/CiscoVT2people_320x192_12fps.yuv 320 192"
  "res/Static_152_100.yuv 152 100"
)

# iRCMode: -1 RC_OFF .. 3 RC_TIMESTAMP. 4 (RC_BITRATE_MODE_POST_SKIP) is rejected
# by InitializeExt in the reference too -- including it reports failures in which
# both encoders exit non-zero.
RCMODES=(-1 0 1 2 3)

# "<slicemode> <slicenum>"; slicenum is the slice count for 1, rows-per-slice for
# 2, and the byte constraint for 3.
SLICES=("1 2" "1 4" "2 3" "3 1500" "3 600")

PASS=0; FAIL=0; FAILED=()

check() {  # label, then compare.sh arguments
  local label=$1; shift
  local out
  out=$(perl -e 'alarm shift; exec @ARGV' "$TIMEOUT" "$CMP" "$@" 2>&1)
  if printf '%s' "$out" | grep -q "BYTE-IDENTICAL"; then
    PASS=$((PASS + 1))
  else
    FAIL=$((FAIL + 1))
    FAILED+=("$label :: $(printf '%s' "$out" | grep -E 'C\+\+ *:|Rust *:|!!' | tr '\n' ' ')")
  fi
}

sweep_st() {
  echo "-- preset: st"
  local YUV W H rc base gop cabac SM SN
  for spec in "${INPUTS[@]}"; do
    read -r YUV W H <<< "$spec"
    for rc in "${RCMODES[@]}"; do
      for base in 0 1; do
        for gop in -1 2 8; do
          for cabac in 0 1; do
            check "st $(basename "$YUV" .yuv) rc=$rc base=$base gop=$gop cabac=$cabac" \
                  "$YUV" "$W" "$H" 5 26 "$cabac" "$gop" "$rc" "$base"
          done
        done
      done
    done
    for slice in "${SLICES[@]}"; do
      read -r SM SN <<< "$slice"
      for cabac in 0 1; do
        check "st $(basename "$YUV" .yuv) sm=$SM n=$SN cabac=$cabac" \
              "$YUV" "$W" "$H" 5 26 "$cabac" -1 0 0 "$SM" "$SN" 1
      done
    done
  done
}

sweep_mt() {
  echo "-- preset: mt"
  local YUV W H thr SM SN cabac rc
  for spec in "${INPUTS[@]}"; do
    read -r YUV W H <<< "$spec"
    for thr in 2 4; do
      for slice in "${SLICES[@]}"; do
        read -r SM SN <<< "$slice"
        for cabac in 0 1; do
          for rc in 0 1; do
            check "mt $(basename "$YUV" .yuv) t=$thr sm=$SM n=$SN cabac=$cabac rc=$rc" \
                  "$YUV" "$W" "$H" 5 26 "$cabac" -1 "$rc" 0 "$SM" "$SN" "$thr"
          done
        done
      done
    done
  done
}

sweep_qp() {
  echo "-- preset: qp"
  local YUV W H qp cabac
  for spec in "${INPUTS[@]}"; do
    read -r YUV W H <<< "$spec"
    for qp in $(seq 0 51); do
      for cabac in 0 1; do
        check "qp $(basename "$YUV" .yuv) qp=$qp cabac=$cabac" \
              "$YUV" "$W" "$H" 3 "$qp" "$cabac" -1
      done
    done
  done
}

[ $# -eq 0 ] && { sed -n '2,14p' "$0"; exit 2; }

for preset in "$@"; do
  case "$preset" in
    st)  sweep_st ;;
    mt)  sweep_mt ;;
    qp)  sweep_qp ;;
    all) sweep_st; sweep_mt; sweep_qp ;;
    *)   echo "unknown preset: $preset" >&2; exit 2 ;;
  esac
done

echo "=========================================="
echo "PASS=$PASS FAIL=$FAIL"
if [ "$FAIL" -gt 0 ]; then
  printf 'FAILURES:\n'
  printf '  %s\n' "${FAILED[@]}"
  exit 1
fi
exit 0
