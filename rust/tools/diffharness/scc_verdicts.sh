#!/bin/bash
# The screen-content **verdict referee** (P10.2.C1).
#
#   usage: rust/tools/diffharness/scc_verdicts.sh                    # the min tier
#          rust/tools/diffharness/scc_verdicts.sh <compare.sh args>  # one row
#
# Why a referee that is not the byte gate. Under `SCC_TIER=min` rate control is
# OFF, so every macroblock on both sides is coded at the driver's QP 26 and
# `iFrameAverageQp` is 26 everywhere. The screen preprocessor's inputs — the
# source frames, which reference slots are available, the reference average QPs —
# are therefore identical on both sides frame by frame *even while the coded bytes
# differ*, because nothing the preprocessor reads depends on the bytes. So once
# the three screen plugins are byte-exact the sequence of scene-change verdicts
# must match frame for frame, and it must match long before P10.3 makes the
# bitstreams match. That is a referee this session can turn green; the byte gate
# is P10.3's.
#
# The observable is the C++'s own DEBUG print, `wels_preprocess.cpp:1247`:
#
#     iVaaFrameSceneChangeIdc = %d,codingIdx = %d
#
# once per P frame, plus — on the LTR rows — the five `WelsBuildRefListScreen()`
# lines of `ref_list_mgr_svc.cpp:811-887`, which say which reference the verdict
# actually selected. Both drivers are asked for `WELS_LOG_DEBUG` through
# `OH264_TRACE_LEVEL=8` (the levels are a bit mask, `codec_app_def.h:323-331`,
# and the sink delivers everything at or below the configured level).
#
# An EMPTY extract on either side is a FAILURE, not a match: a missing observable
# is the state this referee was built to see (the port printed no verdict line at
# all when it was written), and `diff /dev/null /dev/null` would call that a pass.
#
# S58: loudness lives in the exit code — non-zero if any row differs.
#
# Written as bash on purpose, quoted expansions throughout; see sweep.sh's header
# for the zsh word-splitting incident this convention is a scar from.
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
OUT=$HERE/out
mkdir -p "$OUT"
cd "$ROOT" || exit 1

PROFILE=${RUST_ENC_PROFILE:-debug}
RUST_ENC=$HERE/rust_enc/target/$PROFILE/rust_enc
CXX_ENC=$HERE/cxx_enc
TIMEOUT=${TIMEOUT:-600}

for bin in "$CXX_ENC" "$RUST_ENC"; do
  if [ ! -x "$bin" ]; then
    echo "scc_verdicts: $bin missing — run rust/tools/diffharness/build.sh" >&2
    exit 2
  fi
done

# `loopfile`, `screenclip` and the seven `scc` rows, shared with sweep.sh.
# shellcheck source=inputs.sh
. "$HERE/inputs.sh"

PASS=0; FAIL=0; FAILED=()

# One row: the same 23 positional arguments compare.sh takes, run through both
# drivers with the trace knobs on. compare.sh is not modified and not reused —
# the trace logs need names of their own beside the stderr logs it already writes.
verdicts_row() {
  local label=$1; shift
  local YUV=$1 W=$2 H=$3 N=$4 QP=$5 CABAC=$6 GOP=$7
  local RC=${8:-} BASE=${9:-} SLM=${10:-} SLN=${11:-} THR=${12:-} CPX=${13:-}
  local LTR=${14:-} LTRP=${15:-} LTRFB=${16:-} PS=${17:-} DL=${18:-} DN=${19:-}
  local BG=${20:-} SO=${21:-} USAGE=${22:-} LL=${23:-}

  local TAG
  TAG=$(basename "$YUV" .yuv)_${W}x${H}_qp${QP}_cabac${CABAC}_gop${GOP}${RC:+_rc$RC}${BASE:+_base$BASE}${SLM:+_sm$SLM}${SLN:+n$SLN}${THR:+_t$THR}${CPX:+_cx$CPX}${LTR:+_ltr$LTR}${LTRP:+p$LTRP}${LTRFB:+f$LTRFB}${PS:+_ps$PS}${DL:+_dl$DL}${DN:+_dn$DN}${BG:+_bg$BG}${USAGE:+_u$USAGE}${LL:+_ll$LL}

  local c_trace=$OUT/c_$TAG.trace r_trace=$OUT/r_$TAG.trace
  local c_v=$OUT/c_$TAG.verdicts r_v=$OUT/r_$TAG.verdicts

  # Unquoted on purpose, exactly as compare.sh spells it: an empty optional
  # argument must vanish rather than become an empty positional one.
  OH264_TRACE_LEVEL=8 OH264_TRACE_LOG="$c_trace" \
    perl -e 'alarm shift; exec @ARGV' "$TIMEOUT" \
    "$CXX_ENC" "$YUV" "$W" "$H" "$N" "$QP" "$CABAC" "$GOP" "$OUT/cv_$TAG.264" \
    $RC $BASE $SLM $SLN $THR $CPX $LTR $LTRP $LTRFB $PS $DL $DN $BG $SO $USAGE $LL \
    >/dev/null 2>"$OUT/cv_$TAG.log"
  local cxx_rc=$?
  OH264_TRACE_LEVEL=8 OH264_TRACE_LOG="$r_trace" \
    perl -e 'alarm shift; exec @ARGV' "$TIMEOUT" \
    "$RUST_ENC" "$YUV" "$W" "$H" "$N" "$QP" "$CABAC" "$GOP" "$OUT/rv_$TAG.264" \
    $RC $BASE $SLM $SLN $THR $CPX $LTR $LTRP $LTRFB $PS $DL $DN $BG $SO $USAGE $LL \
    >/dev/null 2>"$OUT/rv_$TAG.log"
  local rust_rc=$?

  if [ "$cxx_rc" -ne 0 ] || [ "$rust_rc" -ne 0 ]; then
    echo "  DRIVER $label  (cxx exit $cxx_rc, rust exit $rust_rc)"
    FAIL=$((FAIL + 1))
    FAILED+=("$label :: driver exit cxx=$cxx_rc rust=$rust_rc")
    return 1
  fi

  extract "$c_trace" "$c_v" "$LTR"
  extract "$r_trace" "$r_v" "$LTR"

  if [ ! -s "$c_v" ] || [ ! -s "$r_v" ]; then
    local why="C++ extract $(wc -l <"$c_v" | tr -d ' ') lines, Rust extract $(wc -l <"$r_v" | tr -d ' ') lines"
    echo "  EMPTY  $label  ($why)"
    FAIL=$((FAIL + 1))
    FAILED+=("$label :: EMPTY — $why")
    return 1
  fi

  if diff -q "$c_v" "$r_v" >/dev/null; then
    echo "  OK     $label  ($(wc -l <"$c_v" | tr -d ' ') lines)"
    PASS=$((PASS + 1))
    return 0
  fi

  echo "  DIFFER $label"
  diff "$c_v" "$r_v" | head -12 | sed 's/^/      /'
  FAIL=$((FAIL + 1))
  FAILED+=("$label :: $(diff "$c_v" "$r_v" | grep -c '^[<>]') differing lines")
  return 1
}

# The observables, in trace order, from one driver's trace log into one extract.
#
# `grep -o` on the verdict line drops the `<level>|[OpenH264] this = 0x..., Debug:`
# prefix, whose address differs between the two processes by construction. The
# reference-list lines carry no address in their payload but do carry that same
# prefix, so they are cut with `sed` instead.
extract() {
  local trace=$1 dst=$2 ltr=${3:-0}
  : > "$dst"
  grep -o 'iVaaFrameSceneChangeIdc = -\{0,1\}[0-9]*,codingIdx = -\{0,1\}[0-9]*' "$trace" >> "$dst" 2>/dev/null
  if [ -n "$ltr" ] && [ "$ltr" != "0" ]; then
    sed -n 's/.*Debug:WelsBuildRefListScreen/WelsBuildRefListScreen/p' "$trace" >> "$dst" 2>/dev/null
  fi
  return 0
}

# The min tier's 28 rows — the same seven inputs and the same arguments
# `sweep_scc` builds before its `[ "$tier" = min ] && return`, kept in step by
# taking both from `inputs.sh`, which is the only place either lives.
min_tier() {
  scc_inputs
  local spec YUV W H N name gop cabac
  for spec in "${SCC_INPUTS[@]}"; do
    read -r YUV W H N <<< "$spec"; name=$(basename "$YUV" .yuv)
    for gop in -1 4; do for cabac in 0 1; do
      verdicts_row "scc-min $name gop=$gop cabac=$cabac" \
            "$YUV" "$W" "$H" "$N" 26 "$cabac" "$gop" -1 0 0 1 1 0 0 30 0 0 1 0 0 0 1 0
    done; done
  done
}

if [ "$#" -gt 0 ]; then
  verdicts_row "row $*" "$@"
else
  echo "-- scc verdict referee: the min tier (RC off, QP 26 both sides)"
  min_tier
fi

echo "PASS=$PASS FAIL=$FAIL"
if [ "$FAIL" -gt 0 ]; then
  echo "-- failures"
  for f in "${FAILED[@]}"; do echo "  $f"; done
  exit 1
fi
exit 0
