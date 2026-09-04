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
#             scc   SCREEN_CONTENT_REAL_TIME: 7 inputs x rc x gop x cabac x slices x
#                   threads x LTR (148 configs; SCC_TIER=gate for the 108 the family
#                   gate runs, =min for the 28-row byte tier)
#                   -- P10.1: FAILS by design (every row DIFFER) until P10.3; not in
#                      gates.sh's family list
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

# Stale-driver guard (Phase 9 E3's log, the voided hand-run fault probes):
# sweep.sh does not build — build.sh does — and a driver older than the tree
# being probed refereed nothing while looking green. Sources newer than a
# binary are an error, not a warning (S58: loudness lives in the exit code).
# codec/-side staleness of cxx_enc remains build.sh's business; this checks
# what the incident was: the Rust tree and the harness's own driver source.
# Override for a deliberately old driver: SWEEP_STALE_OK=1.
#
# **`$HERE/rust_enc` is the cargo crate DIRECTORY, not the binary** — the binary
# is `$HERE/rust_enc/target/$PROFILE/rust_enc`, which is how `compare.sh:35`
# spells it and how this guard must. As first written (`583cd21a`) it tested the
# directory: `[ -x <dir> ]` is true, so the missing-binary arm never fired, and
# the directory's mtime is when a file was last added to the crate — Aug 22 —
# so `find -newer` matched 83 sources and the guard reported STALE on every run,
# including inside `gates.sh`, whose two sweeps it failed with rc=2 from the
# moment it landed. It was calibrated on a positive and never on a negative
# (S55's other half): it was shown to fire, never shown to stay silent, and its
# commit message read the false positive as a true one. Fixed and calibrated
# both ways in Phase 9 session G (F159); the profile is now part of the answer,
# because a fresh debug driver says nothing about a stale release one.
if [ "${SWEEP_STALE_OK:-0}" != "1" ]; then
  stale=""
  _prof=${RUST_ENC_PROFILE:-debug}
  _renc="$HERE/rust_enc/target/$_prof/rust_enc"
  if [ ! -f "$_renc" ] || [ ! -x "$_renc" ]; then
    stale="rust_enc ($_prof) missing: $_renc"
  elif [ ! -f "$HERE/cxx_enc" ] || [ ! -x "$HERE/cxx_enc" ]; then
    stale="cxx_enc missing: $HERE/cxx_enc"
  else
    newer=$(find "$ROOT/rust/crates/openh264-rs/src" -name '*.rs' -newer "$_renc" 2>/dev/null | head -3)
    [ -n "$newer" ] && stale="rust_enc ($_prof) older than: $(echo "$newer" | tr '\n' ' ')"
    [ "$HERE/cxx_enc.cpp" -nt "$HERE/cxx_enc" ] && stale="${stale:+$stale; }cxx_enc older than cxx_enc.cpp"
  fi
  if [ -n "$stale" ]; then
    echo "sweep.sh: STALE DRIVER — $stale" >&2
    echo "sweep.sh: run rust/tools/diffharness/build.sh first (or SWEEP_STALE_OK=1 to override)" >&2
    exit 2
  fi
fi

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

# `loopfile`, `screenclip` and the seven `scc` rows — shared with
# `scc_verdicts.sh` since P10.2.C1, which needs the same inputs and cannot source
# this script (it exits 2 with no preset and resets the tallies its sourcer keeps).
# shellcheck source=inputs.sh
. "$HERE/inputs.sh"

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
  # `t=4` is not optional. `WelsMdBackgroundMbEnc` runs *in-fork*, and
  # `VaaBackgroundMbDataUpdate` writes the current source picture through raw
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

sweep_scc() {
  echo "-- preset: scc"
  local tier=${SCC_TIER:-all}
  local YUV W H N name rc gop cabac thr
  # P10.1: the screen-content axis, byte-identical since P10.3's dispatch block —
  # 148/148 in both profiles. **In `gates.sh`'s family list since P10.4 (D-scc-17)**,
  # the whole preset rather than `SCC_TIER=min`, because the full 148 rows measured
  # 51s debug / 27s release and the ruling's threshold was three minutes. Before
  # P10.3 every row FAILED and the preset's job was to fail for the recorded reason
  # (a Rust init failure before P10.1.B5, byte differences with every frame encoded
  # after), which is why it sat outside the gate while it existed.
  #
  # **The three tiers** (`SCC_TIER`, default `all`):
  #   all   148 rows — the whole preset, what `sweep.sh all` and the phase exit run.
  #   gate  108 rows — `all` minus the 40 `sm=3 t=4` rows. **This is what
  #         `gates.sh family` runs, and F334 is why.** Those 40 rows sample a race
  #         in the **C++ reference**, not in the port: 100 alternating solo runs of
  #         one of them (`Static_152_100_loop60 rc=1 gop=4 cabac=0`) gave the port
  #         100 identical bitstreams and the reference **12 distinct ones**, the
  #         port's single output being the reference's own 88-run majority. It is
  #         F3's `SM_SIZELIMITED_SLICE` + multithreading race, whose window screen
  #         usage widens by about two orders of magnitude — the same reference
  #         binary was 40/40 deterministic on the camera version of the row and
  #         40/40 on the screen `sm=0 t=4` LTR row. At that rate every family run
  #         would go red on a reference misencode, so the gate does not sample it,
  #         exactly as `abi_harness/run.sh` already refuses to ("a gate is not the
  #         place to sample a known flake"). The rows stay in `all`, under the F3
  #         retry rule.
  #   min    28 rows — RC off, single slice, one thread: P10.3's first byte gate,
  #         and the tier `scc_verdicts.sh` referees.
  #
  # Every row passes `so = 0` (compare.sh's 21st argument) before `usage = 1`:
  # driver arguments are positional, and a row that omitted it would shift `usage`
  # into the setoptext slot and encode camera content with a SetOption after frame
  # 1 — two identical camera streams the sweep would happily call a PASS.
  # The seven rows live in inputs.sh, so `scc_verdicts.sh` refereeing the min tier
  # and this preset encoding it cannot drift apart (P10.2.C1).
  scc_inputs
  local INPUTS_SCC=("${SCC_INPUTS[@]}")

  # tier min (28 rows): RC off, single slice, one thread, no LTR — P10.3's first byte gate.
  for spec in "${INPUTS_SCC[@]}"; do
    read -r YUV W H N <<< "$spec"; name=$(basename "$YUV" .yuv)
    for gop in -1 4; do for cabac in 0 1; do
      check "scc-min $name gop=$gop cabac=$cabac" \
            "$YUV" "$W" "$H" "$N" 26 "$cabac" "$gop" -1 0 0 1 1 0 0 30 0 0 1 0 0 0 1 0
    done; done
  done
  [ "$tier" = min ] && return

  # tier wide (120 rows) on five inputs: the three res clips and two synthetic ones.
  local WIDE=("${INPUTS_SCC[0]}" "${INPUTS_SCC[1]}" "${INPUTS_SCC[2]}" "${INPUTS_SCC[3]}" "${INPUTS_SCC[6]}")
  for spec in "${WIDE[@]}"; do
    read -r YUV W H N <<< "$spec"; name=$(basename "$YUV" .yuv)
    # rate control x gop x cabac x {single slice/1 thread, size-limited slices/4 threads}
    for rc in 1 2; do for gop in -1 4; do for cabac in 0 1; do
      check "scc $name rc=$rc gop=$gop cabac=$cabac sm=0 t=1" \
            "$YUV" "$W" "$H" "$N" 26 "$cabac" "$gop" "$rc" 0 0 1 1 0 0 30 0 0 1 0 0 0 1 0
      # F334: the reference's own race lives here — excluded from the `gate` tier,
      # kept in `all`. See the tier block at the top of this function.
      [ "$tier" = gate ] || \
      check "scc $name rc=$rc gop=$gop cabac=$cabac sm=3 t=4" \
            "$YUV" "$W" "$H" "$N" 26 "$cabac" "$gop" "$rc" 0 3 1500 4 0 0 30 0 0 1 0 0 0 1 0
    done; done; done
    # long-term reference over a lossless link (the CWelsReference_LosslessWithLtr path)
    for rc in -1 2; do for cabac in 0 1; do for thr in 1 4; do
      check "scc $name rc=$rc cabac=$cabac t=$thr ltr=4 lossless" \
            "$YUV" "$W" "$H" "$N" 26 "$cabac" -1 "$rc" 0 0 1 "$thr" 0 4 30 0 0 1 0 0 0 1 1
    done; done; done
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
    scc) sweep_scc ;;
    all) sweep_st; sweep_mt; sweep_qp; sweep_def; sweep_sl; sweep_ltr; sweep_ps; sweep_dl; sweep_bg; sweep_scc ;;
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
