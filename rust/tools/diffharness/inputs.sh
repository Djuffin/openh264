# Shared input builders for the differential harness — sourced, never run.
#
# `sweep.sh` owned both of these until P10.2.C1 gave `scc_verdicts.sh` a second
# caller for the same seven screen-content rows. Sourcing `sweep.sh` itself is not
# an option: it exits 2 when given no preset, and resets the PASS/FAIL tallies its
# sourcer is keeping. So the two builders live here and both scripts source this.
#
# Requires `HERE` (the harness directory) and a working directory of the
# repository root, which both callers set before sourcing.
#
# Written as bash, quoted expansions; see sweep.sh's header for why that matters.

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
  local raw_sz
  raw_sz=$(stat -c%s "$yuv" 2>/dev/null || stat -f%z "$yuv" 2>/dev/null || wc -c < "$yuv")
  nsrc=$((raw_sz / fsz))
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

# Build (once, cached in out/) a synthetic screen clip; see gen_screen_clip.py.
#
# The res/ clips are camera video: their text-free, unscrolled frames seldom satisfy
# the scroll detector's line test (`CheckLine`, `ScrollDetectionFuncs.cpp:37`) and
# never scroll by whole rows, so without these clips nothing in Phase 10 would ever
# exercise `JudgeScrollSkip`, `WelsMotionEstimateSearchScrolled`,
# `SetScrollingMvToMd` or `CheckDirectionalMv`. Reported in SCREEN_PATH.
SCREEN_PATH=""
screenclip() {  # name w h n k c d seed
  SCREEN_PATH="$HERE/out/$1.yuv"
  if [ ! -f "$SCREEN_PATH" ]; then
    mkdir -p "$HERE/out"
    python3 "$HERE/gen_screen_clip.py" --width "$2" --height "$3" --frames "$4" \
      --scroll "$5" --cut-every "$6" --hold-every "$7" --seed "$8" --out "$SCREEN_PATH"
  fi
}

# The seven `scc` inputs, in the order `sweep_scc` and `scc_verdicts.sh` index
# them, reported in the global array SCC_INPUTS. Each element is
# "<path> <w> <h> <frames>", the shape both callers feed to `read -r YUV W H N`.
#
# A fixed global rather than an out-parameter because the shell here is bash 3.2
# (macOS), which has no `local -n`.
#
# 152 is not a multiple of 16, so the third row also exercises the aligned-and-
# cropped path under screen usage.
SCC_INPUTS=()
scc_inputs() {
  SCC_INPUTS=()
  loopfile res/CiscoVT2people_160x96_6fps.yuv 160 96 60;    SCC_INPUTS+=("$LOOP_PATH 160 96 $LOOP_FRAMES")
  loopfile res/CiscoVT2people_320x192_12fps.yuv 320 192 60; SCC_INPUTS+=("$LOOP_PATH 320 192 $LOOP_FRAMES")
  loopfile res/Static_152_100.yuv 152 100 60;               SCC_INPUTS+=("$LOOP_PATH 152 100 $LOOP_FRAMES")
  screenclip scc_text_320x192_k3  320 192 60 3  20 7 1;     SCC_INPUTS+=("$SCREEN_PATH 320 192 60")
  screenclip scc_text_320x192_k17 320 192 60 17 20 0 2;     SCC_INPUTS+=("$SCREEN_PATH 320 192 60")
  screenclip scc_text_160x96_k1   160 96  60 1  0  7 3;     SCC_INPUTS+=("$SCREEN_PATH 160 96 60")
  screenclip scc_text_640x368_k8  640 368 40 8  20 7 4;     SCC_INPUTS+=("$SCREEN_PATH 640 368 40")
}
