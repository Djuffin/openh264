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
#             def   GetDefaultParams + InitializeExt (baseinit=2) on inputs
#                   looped to 72+ frames x threads 1/2/4, plus 720p x 1/4
#                                                                        (11 configs)
#             all   every preset above
#
# st and mt encode SWEEP_FRAMES (default 16, rounded up to 18-20 by looping) frames
# per configuration; qp stays at 3, since it sweeps quantiser breadth rather than
# sequence depth. See `loopfile` for why the frame count matters more than it looks.
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

# Build (once, cached in out/) a looped copy of $1 holding at least $4 frames, and
# report it in LOOP_PATH / LOOP_FRAMES.
#
# The res/ clips are 5, 9 and 10 frames — shorter than the encoder's own state
# cycles, so passing a larger frame count on its own changes nothing: both drivers
# stop at EOF. Rate control works in 8-frame VGOPs, and several defect classes only
# surface at the second IDR, so a 5-frame comparison exercises far less of the
# encoder than the config count suggests. The GetDefaultParams divergence fixed in
# fa67432f sat undetected behind 330 passing 5-frame configurations.
LOOP_PATH=""; LOOP_FRAMES=0
loopfile() {
  local yuv=$1 w=$2 h=$3 want=$4
  local fsz nsrc reps i
  fsz=$((w * h * 3 / 2))
  nsrc=$(($(stat -f%z "$yuv") / fsz))
  reps=$(((want + nsrc - 1) / nsrc))
  LOOP_FRAMES=$((nsrc * reps))
  LOOP_PATH="$HERE/out/$(basename "$yuv" .yuv)_loop${LOOP_FRAMES}.yuv"
  if [ ! -f "$LOOP_PATH" ]; then
    mkdir -p "$HERE/out"
    : > "$LOOP_PATH"
    i=0
    while [ "$i" -lt "$reps" ]; do cat "$yuv" >> "$LOOP_PATH"; i=$((i + 1)); done
  fi
}

# Frames per configuration for st/mt. 16 clears a full 8-frame VGOP plus the IDR
# that opens the sequence; looping rounds it up to 18-20 depending on the clip.
ST_FRAMES=${SWEEP_FRAMES:-16}

sweep_st() {
  echo "-- preset: st"
  local YUV W H rc base gop cabac SM SN name
  for spec in "${INPUTS[@]}"; do
    read -r YUV W H <<< "$spec"
    name=$(basename "$YUV" .yuv)
    loopfile "$YUV" "$W" "$H" "$ST_FRAMES"
    for rc in "${RCMODES[@]}"; do
      for base in 0 1; do
        for gop in -1 2 8; do
          for cabac in 0 1; do
            check "st $name rc=$rc base=$base gop=$gop cabac=$cabac" \
                  "$LOOP_PATH" "$W" "$H" "$LOOP_FRAMES" 26 "$cabac" "$gop" "$rc" "$base"
          done
        done
      done
    done
    for slice in "${SLICES[@]}"; do
      read -r SM SN <<< "$slice"
      for cabac in 0 1; do
        check "st $name sm=$SM n=$SN cabac=$cabac" \
              "$LOOP_PATH" "$W" "$H" "$LOOP_FRAMES" 26 "$cabac" -1 0 0 "$SM" "$SN" 1
      done
    done
  done
}

sweep_mt() {
  echo "-- preset: mt"
  local YUV W H thr SM SN cabac rc name
  for spec in "${INPUTS[@]}"; do
    read -r YUV W H <<< "$spec"
    name=$(basename "$YUV" .yuv)
    loopfile "$YUV" "$W" "$H" "$ST_FRAMES"
    for thr in 2 4; do
      for slice in "${SLICES[@]}"; do
        read -r SM SN <<< "$slice"
        for cabac in 0 1; do
          for rc in 0 1; do
            check "mt $name t=$thr sm=$SM n=$SN cabac=$cabac rc=$rc" \
                  "$LOOP_PATH" "$W" "$H" "$LOOP_FRAMES" 26 "$cabac" -1 "$rc" 0 "$SM" "$SN" "$thr"
          done
        done
      done
    done
  done
}

# GetDefaultParams + InitializeExt with only width/height/framerate/bitrate/threads
# set on top (baseinit=2) — FillDefault's real values, so frame skip, adaptive
# quantisation, scene-change/background detection and bFixRCOverShoot are all ON.
# The res/ clips are 5-10 frames, and the defect classes this path has produced
# (VGOP deficit carry-over, second-IDR budget) first bite at the 10th coded frame
# and the second IDR — so every clip is looped out to 40+ frames first. Short
# inputs are exactly why this axis went untested for so long.
sweep_def() {
  echo "-- preset: def"
  local YUV W H thr name want
  for spec in "${INPUTS[@]}" "res/Cisco_Absolute_Power_1280x720_30fps.yuv 1280 720"; do
    read -r YUV W H <<< "$spec"
    name=$(basename "$YUV" .yuv)
    # 72+ frames for the small clips; 40 is enough at 720p and keeps runtime sane.
    if [ "$W" -ge 1280 ]; then want=40; else want=72; fi
    loopfile "$YUV" "$W" "$H" "$want"
    # qp/cabac/gop/rcmode/slice arguments are ignored by the drivers in this
    # mode; they are passed only to reach the threads position.
    if [ "$W" -ge 1280 ]; then
      for thr in 1 4; do
        check "def $name t=$thr" "$LOOP_PATH" "$W" "$H" "$LOOP_FRAMES" 26 0 -1 0 2 0 1 "$thr"
      done
    else
      for thr in 1 2 4; do
        check "def $name t=$thr" "$LOOP_PATH" "$W" "$H" "$LOOP_FRAMES" 26 0 -1 0 2 0 1 "$thr"
      done
    fi
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
    def) sweep_def ;;
    all) sweep_st; sweep_mt; sweep_qp; sweep_def ;;
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
