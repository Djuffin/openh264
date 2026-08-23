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
#             sl    SM_SIZELIMITED_SLICE at constraints tight enough to cross
#                   iMaxSliceNum and drive the slice-realloc path         (12 configs)
#             ltr   bEnableLongTermReference on, x LTR feedback bitmask
#                   x intra period                                        (16 configs)
#             ps    all 5 eSpsPpsIdStrategy values x cabac x GOP x input  (90 configs)
#             dl    iSpatialLayerNum 2/3/4 x denoise on/off x GOP x cabac x
#                   input, plus 720p x layers 2/4 x denoise                (76 configs)
#                   -- measured 76/76 at T8b.C2; ps measured 90/90 the same day,
#                      the first time it had ever been run
#                   -- the only preset that runs METHOD_DOWNSAMPLE at all
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

# `sl` rows: "<qp> <slice byte constraint>". SM_SIZELIMITED_SLICE only.
#
# The slice-realloc path (`FrameBsRealloc`/`ExtendLayerBuffer`, svc_encode_slice.rs)
# runs only when a frame's coded slice count crosses `iMaxSliceNum`, which opens at
# 35. Every other preset here tops out around 9 coded slices, so that path had no
# standing byte coverage at all — session D found and fixed a re-aim bug in it
# out-of-band, against a hand-run comparison that then evaporated.
#
# These three pairs at 320x192 (20x12 = 240 macroblocks) each cross 35 slices in a
# frame, and all 12 rows below were measured entering `FrameBsRealloc` at least once
# (Phase 6 session E, 2026-08-19; probe = an eprintln at the function's head).
# The constraint floor is 401: `ParamValidationExt` rejects anything <=
# MAX_MACROBLOCK_SIZE_IN_BYTE (400).
#
# rc modes -1 (RC_OFF) and 2 (RC_BUFFERBASED) are the two that reach it. 0/1/3 hold
# the frame budget low enough that the slice count stays under 35 whatever the qp
# argument says, which is why this preset names its rc modes rather than looping
# RCMODES.
#
# The three constraints must differ from each other, not just the three qps: under
# rc=2 the rate controller picks the quantiser and the qp column is inert, so two
# rows sharing a constraint would be the same encode twice. (They were, in this
# preset's first draft: "26 401" and "10 401" produced byte-identical streams under
# rc=2.) The constraint is the only axis both rc modes actually read.
SL_ROWS=("26 401" "16 601" "10 501")

# `ltr` rows: "<gop> <ltrfb>". bEnableLongTermReference is ON for all of them.
#
# Long-term reference had **no byte coverage at all** until Phase 6 session F: both
# drivers hard-coded `bEnableLongTermReference = false`, so `LTRMarkProcess`,
# `DeleteInvalidLTR`, `DeleteLTRFromLongList`, `HandleLTRMarkFeedback`,
# `FilterLTRMarkingFeedback`, `WelsBuildRefList`'s long-reference arm and every
# `pLongRefList` shift were unreachable — and the picture-id flip (T6.F1) rewrites
# all of them. Session E's `sl` is the precedent, F60's silent divergence the reason.
#
# `ltrfb` is a bitmask over the two feedback packets a real application relays from
# its decoder — 1 = ENCODER_LTR_MARKING_FEEDBACK, 2 = ENCODER_LTR_RECOVERY_REQUEST.
# They are not decoration: without bit 1 `DeleteLTRFromLongList` never runs, and
# without bit 2 `bReceivedT0LostFlag` is never set, so `WelsBuildRefList` never takes
# its long-reference arm and `SetRefMbType` never takes its long half. Each of the
# four values produces a *different* stream (measured: 223062 / 223075 / 229470 /
# 236572 bytes at gop=0 on the 320x192 clip), so the axis is real, not inert.
#
# The `ltr` argument's own value is inert by design and 2 is spelled for honesty
# rather than effect: `WelsCheckNumRefSetting` (au_set.cpp:92) resets iLTRRefNum to
# LONG_TERM_REF_NUM = 2 for camera content whatever the caller asked for.
LTR_ROWS=("0 0" "0 1" "0 2" "0 3" "8 0" "8 1" "8 2" "8 3")

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

sweep_sl() {
  echo "-- preset: sl"
  local YUV="res/CiscoVT2people_320x192_12fps.yuv" W=320 H=192
  local name qp con cabac rc
  name=$(basename "$YUV" .yuv)
  loopfile "$YUV" "$W" "$H" "$ST_FRAMES"
  for row in "${SL_ROWS[@]}"; do
    read -r qp con <<< "$row"
    for rc in -1 2; do
      for cabac in 0 1; do
        check "sl $name qp=$qp con=$con rc=$rc cabac=$cabac" \
              "$LOOP_PATH" "$W" "$H" "$LOOP_FRAMES" "$qp" "$cabac" -1 "$rc" 0 3 "$con" 1
      done
    done
  done
}

sweep_ltr() {
  echo "-- preset: ltr"
  local YUV W H gop fb name
  for spec in "res/CiscoVT2people_160x96_6fps.yuv 160 96" \
              "res/CiscoVT2people_320x192_12fps.yuv 320 192"; do
    read -r YUV W H <<< "$spec"
    name=$(basename "$YUV" .yuv)
    # LTR state cycles over several GOPs — the mark/confirm/delete round trip needs
    # more frames than st's 16 to complete even once.
    loopfile "$YUV" "$W" "$H" 72
    for row in "${LTR_ROWS[@]}"; do
      read -r gop fb <<< "$row"
      check "ltr $name gop=$gop fb=$fb" \
            "$LOOP_PATH" "$W" "$H" "$LOOP_FRAMES" 26 1 "$gop" -1 0 0 1 1 0 2 4 "$fb"
    done
  done
}

# The five `eSpsPpsIdStrategy` values (Phase 8b session B, T8b.B3). The three
# listing strategies refused at `InitializeExt` until that session, so this axis had
# never been swept at all; the five pairs run by hand there are its first four rows.
#
# The GOP axis is what makes it bite: `uiIntraPeriod` is how often an IDR — and so a
# parameter-set write — happens, and the strategies differ only at those writes. A
# single-IDR configuration makes `SPS_LISTING` and `CONSTANT_ID` produce identical
# bytes, which is correct and proves nothing.
#
# **What this preset still cannot reach**: a mid-stream `InitializeExt` with changed
# parameters, which is where `SPS_LISTING` actually re-uses a stored SPS rather than
# matching the only one there is. That needs a `--reinit-at` knob in both drivers;
# T8b.B3 dropped it under the brief's step 5 and left the six
# `EncodeDecodeTestAPI.ParameterSetStrategy_*` gtest rows as its referee, since their
# bodies re-initialise exactly that way.
sweep_ps() {
  echo "-- preset: ps"
  local YUV W H st gop cabac name
  for spec in "${INPUTS[@]}"; do
    read -r YUV W H <<< "$spec"
    name=$(basename "$YUV" .yuv)
    loopfile "$YUV" "$W" "$H" "$ST_FRAMES"
    # 0 CONSTANT_ID, 1 INCREASING_ID, 2 SPS_LISTING,
    # 3 SPS_LISTING_AND_PPS_INCREASING, 6 SPS_PPS_LISTING — the enum's own values,
    # which are not a dense range (`codec_app_def.h:514-518`).
    for st in 0 1 2 3 6; do
      for gop in -1 4 1; do
        for cabac in 0 1; do
          check "ps $name strategy=$st gop=$gop cabac=$cabac" \
                "$LOOP_PATH" "$W" "$H" "$LOOP_FRAMES" 26 "$cabac" "$gop" 0 0 0 1 1 0 0 30 0 "$st"
        done
      done
    done
  done
}

sweep_dl() {
  echo "-- preset: dl"
  local YUV W H n dn gop cabac name
  # **Dependency layers — the preset that exercises `METHOD_DOWNSAMPLE`, and with
  # `dn=1` `METHOD_DENOISE` alongside it (Phase 8b session C, T8b.C2).** Until that
  # session the port refused both at `InitializeExt` (S48), so neither had a single
  # byte of coverage; 17 gtest rows were allowlisted behind the pair.
  #
  # Layer geometry is `BaseEncoderTest`'s own (`test/api/BaseEncoderTest.cpp:43`):
  # layer i is the input halved `n - 1 - i` times. So `n` here is also the number of
  # cascaded halvings the downsampler performs, and `n=4` at 1280x720 is the case
  # that distinguishes a correct port from an obvious-but-wrong one — see F98: the
  # reference reaches 4:1 by halving *twice through a scratch buffer*, not by the
  # quarter kernel a reading of `Process`'s first arm would suggest.
  #
  # **No CPU-flag forcing here, and that is a measured decision, not an oversight.**
  # `libopenh264.a` dispatches to AArch64 NEON downsamplers and the port translates
  # the `_c` ones; `rust/tools/vp_kernel_probe/` shows the two are bit-identical on
  # every kernel with a sibling (F97), so `cxx_enc` is a fair referee as it stands.
  # What is *not* interchangeable is the table: aarch64 binds general-ratio luma to
  # the accurate wrapper where the scalar table binds the fast one, and the port
  # follows aarch64.
  for spec in "${INPUTS[@]}"; do
    read -r YUV W H <<< "$spec"
    name=$(basename "$YUV" .yuv)
    loopfile "$YUV" "$W" "$H" "$ST_FRAMES"
    for n in 2 3 4; do
      for dn in 0 1; do
        for gop in -1 4; do
          for cabac in 0 1; do
            check "dl $name layers=$n denoise=$dn gop=$gop cabac=$cabac" \
                  "$LOOP_PATH" "$W" "$H" "$LOOP_FRAMES" 26 "$cabac" "$gop" 0 0 0 1 1 0 0 30 0 0 "$n" "$dn"
          done
        done
      done
    done
  done
  # 720p, where the halvings actually cascade: 1280 -> 640 -> 320 -> 160.
  loopfile "res/Cisco_Absolute_Power_1280x720_30fps.yuv" 1280 720 6
  for n in 2 4; do
    for dn in 0 1; do
      check "dl 720p layers=$n denoise=$dn" \
            "$LOOP_PATH" 1280 720 "$LOOP_FRAMES" 26 0 -1 0 0 0 1 1 0 0 30 0 0 "$n" "$dn"
    done
  done
}

sweep_bg() {
  echo "-- preset: bg"
  local YUV W H rc thr gop cabac name
  # **Background detection — the family that had no byte referee at all (Phase 9
  # session B4, D-ref-1).** `WelsInitBGDFunc` (`encoder_context.rs:1606`) installs
  # `pfInterMdBackgroundDecision` = `WelsMdInterJudgeBGDPskip` only behind
  # `bEnableBackgroundDetection`, and every driver before this session pinned that
  # flag `false`. `FillDefault` leaves it ON, so ordinary applications run
  # `WelsMdBackgroundMbEnc`, `VaaBackgroundMbDataUpdate`, `WelsMdUpdateBGDInfo` and
  # the analyzer's `BackgroundDetection` on every P slice and this harness ran none
  # of them: a probe read 0 entries across five sweep configurations (F117/T9.B27).
  #
  # **What the rows have to satisfy for the axis to be real.** Two gates sit between
  # the flag and `WelsMdBackgroundMbEnc`:
  #   * `AnalyzeSpatialPic` computes `bCalculateBGD` as `eSliceType == P_SLICE &&
  #     bEnableBackgroundDetection` (`wels_preprocess.rs:1359`), so an all-IDR
  #     configuration marks nothing. Every row below leaves `gop` at -1 or 4, never 0.
  #   * `WelsMdInterJudgeBGDPskip` enters the encode only where
  #     `pVaaBackgroundMbFlag != 0` for that macroblock, which the analyzer sets only
  #     for genuinely static blocks. The content therefore has to *have* a
  #     background: `Static_152_100` is the strongest case and the two `CiscoVT2people`
  #     talking-head clips are the realistic one. A clip with no static region would
  #     pass every row while entering nothing, which is the failure mode this comment
  #     exists to prevent — calibrate with a probe (S55) before trusting a PASS here.
  #
  # 72 frames, not `ST_FRAMES`: `pVaaBackgroundMbFlag` is a per-frame decision fed by
  # the *previous* frame's reconstruction, and the collocated-QP arm
  # (`kiRefMbQp - kiCurMbQp <= DELTA_QP_BGD_THD`) only starts discriminating once rate
  # control has moved the QP around, which takes several VGOPs.
  #
  # `t=4` is not optional. `WelsMdBackgroundMbEnc` runs *in-fork* (the census marks it
  # so), and `VaaBackgroundMbDataUpdate` writes the current source picture through raw
  # roots from inside a slice thread; a single-threaded-only preset would referee the
  # arithmetic and none of the threading.
  for spec in "res/Static_152_100.yuv 152 100" \
              "res/CiscoVT2people_160x96_6fps.yuv 160 96" \
              "res/CiscoVT2people_320x192_12fps.yuv 320 192"; do
    read -r YUV W H <<< "$spec"
    name=$(basename "$YUV" .yuv)
    loopfile "$YUV" "$W" "$H" 72
    for rc in -1 2; do
      for gop in -1 4; do
        for cabac in 0 1; do
          for thr in 1 4; do
            check "bg $name rc=$rc gop=$gop cabac=$cabac t=$thr" \
                  "$LOOP_PATH" "$W" "$H" "$LOOP_FRAMES" 26 "$cabac" "$gop" "$rc" 0 0 1 "$thr" 0 0 30 0 0 1 0 1
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

[ $# -eq 0 ] && { sed -n '2,19p' "$0"; exit 2; }

for preset in "$@"; do
  case "$preset" in
    st)  sweep_st ;;
    mt)  sweep_mt ;;
    qp)  sweep_qp ;;
    def) sweep_def ;;
    sl)  sweep_sl ;;
    ltr) sweep_ltr ;;
    ps)  sweep_ps ;;
    dl)  sweep_dl ;;
    bg)  sweep_bg ;;
    all) sweep_st; sweep_mt; sweep_qp; sweep_def; sweep_sl; sweep_ltr; sweep_ps; sweep_dl; sweep_bg ;;
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
