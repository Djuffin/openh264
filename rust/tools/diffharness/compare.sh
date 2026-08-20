#!/bin/bash
# Differential comparison: C++ reference encoder vs the Rust port, byte for byte.
#
#   usage: rust/tools/diffharness/compare.sh <yuv> <w> <h> <frames> <qp> <cabac> <gop> [rcmode] [baseinit] [slicemode] [slicenum] [threads] [complexity] [ltr] [ltrperiod] [ltrfb]
#
#   slicemode: 0 SM_SINGLE_SLICE (default), 1 SM_FIXEDSLCNUM_SLICE,
#              2 SM_RASTER_SLICE, 3 SM_SIZELIMITED_SLICE.
#   slicenum:  slice count for 1/2, rows-per-slice for 2, byte constraint for 3.
#   RUST_ENC_PROFILE=debug (default) | release picks which build of rust_enc to
#              run; build.sh reads the same variable. Both profiles must produce
#              identical bytes — see build.sh for why that is a gate and not a
#              formality. The two profiles share out/ file names, so run them one
#              after another rather than concurrently.
#
#   baseinit:  0 InitializeExt with the fully explicit gate config (default),
#              1 Initialize(SEncParamBase), 2 GetDefaultParams + InitializeExt
#              with only width/height/framerate/bitrate/threads set on top (the
#              ordinary API flow; qp/cabac/gop/rcmode/slice args are ignored).
#   e.g.   rust/tools/diffharness/compare.sh res/CiscoVT2people_160x96_6fps.yuv 160 96 5 26 0 -1
#
# Both drivers set a fully explicit, identical SEncParamExt, so the only
# variable under test is the encoder implementation. Exits 0 iff the two
# bitstreams are byte-identical.
#
# Prerequisites (see rust/docs/encoder_port_status.md):
#   make -j8 libraries binaries      # builds libopenh264.a and h264dec
#   rust/tools/diffharness/build.sh  # builds both drivers
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
OUT=$HERE/out
mkdir -p "$OUT"

PROFILE=${RUST_ENC_PROFILE:-debug}
RUST_ENC=$HERE/rust_enc/target/$PROFILE/rust_enc

YUV=$1; W=$2; H=$3; N=$4; QP=$5; CABAC=$6; GOP=$7; RC=${8:-}; BASE=${9:-}
SLM=${10:-}; SLN=${11:-}; THR=${12:-}
# 13th: iComplexityMode. 0 LOW (every sweep configuration, and the default), 1 MEDIUM,
# 2 HIGH. LOW is `bFastMode`, which selects a *different mode-decision family*
# (`SetFastCodingFunc` vs `SetNormalCodingFunc`, `encoder_ext.cpp:2665`) — so the
# fine I4x4 partition search and its prediction ping-pong were reachable by no
# configuration this harness could express until Phase 6 session C added this knob.
CPX=${13:-}
# 14th/15th: iLTRRefNum and iLtrMarkPeriod. 0 (the default) leaves long-term
# reference OFF — which is what every configuration this harness could express ran
# until Phase 6 session F added the knob, so `WelsUpdateRefList`'s long-term half,
# `LTRMarkProcess`, `DeleteLTRFromLongList` and every `pLongRefList` shift had no
# byte coverage at all. See the `ltr` preset in sweep.sh.
LTR=${14:-}; LTRP=${15:-}; LTRFB=${16:-}
TAG=$(basename "$YUV" .yuv)_${W}x${H}_qp${QP}_cabac${CABAC}_gop${GOP}${RC:+_rc$RC}${BASE:+_base$BASE}${SLM:+_sm$SLM}${SLN:+n$SLN}${THR:+_t$THR}${CPX:+_cx$CPX}${LTR:+_ltr$LTR}${LTRP:+p$LTRP}${LTRFB:+f$LTRFB}

cd "$ROOT" || exit 1
"$HERE/cxx_enc"                        "$YUV" "$W" "$H" "$N" "$QP" "$CABAC" "$GOP" "$OUT/c_$TAG.264"  $RC $BASE $SLM $SLN $THR $CPX $LTR $LTRP $LTRFB 2>"$OUT/c_$TAG.log"
cxx_rc=$?
"$RUST_ENC"                            "$YUV" "$W" "$H" "$N" "$QP" "$CABAC" "$GOP" "$OUT/r_$TAG.264" $RC $BASE $SLM $SLN $THR $CPX $LTR $LTRP $LTRFB 2>"$OUT/r_$TAG.log"
rust_rc=$?

# A driver that aborts leaves a short file, which otherwise reads as an ordinary
# byte difference. Say so: a debug-build Rust panic (an arithmetic overflow the
# C++ wraps through, say) is a different defect from a wrong bit.
[ "$cxx_rc"  -ne 0 ] && echo "  !! cxx_enc  exited $cxx_rc  — see $OUT/c_$TAG.log"
[ "$rust_rc" -ne 0 ] && echo "  !! rust_enc exited $rust_rc — see $OUT/r_$TAG.log"

cs=$(stat -f%z "$OUT/c_$TAG.264" 2>/dev/null || echo -1)
rs=$(stat -f%z "$OUT/r_$TAG.264" 2>/dev/null || echo -1)
echo "=== $TAG ==="
echo "  C++  : $cs bytes"
echo "  Rust : $rs bytes"

if cmp -s "$OUT/c_$TAG.264" "$OUT/r_$TAG.264"; then
  echo "  RESULT: BYTE-IDENTICAL"
  rc=0
else
  echo "  RESULT: DIFFER"
  cmp "$OUT/c_$TAG.264" "$OUT/r_$TAG.264" 2>&1 | head -3 | sed 's/^/    /'
  rc=1
fi

# Sanity: does the Rust stream decode with the reference decoder?
if [ "$rs" -gt 0 ]; then
  "$ROOT/h264dec" "$OUT/r_$TAG.264" "$OUT/r_$TAG.yuv" >"$OUT/r_$TAG.dec.log" 2>&1
  echo "  Rust stream decodes to $(stat -f%z "$OUT/r_$TAG.yuv" 2>/dev/null || echo 0) bytes YUV"
fi
exit $rc
