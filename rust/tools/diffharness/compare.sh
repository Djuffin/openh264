#!/bin/bash
# Differential comparison: C++ reference encoder vs the Rust port, byte for byte.
#
#   usage: rust/tools/diffharness/compare.sh <yuv> <w> <h> <frames> <qp> <cabac> <gop> [rcmode]
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

YUV=$1; W=$2; H=$3; N=$4; QP=$5; CABAC=$6; GOP=$7; RC=${8:-}
TAG=$(basename "$YUV" .yuv)_${W}x${H}_qp${QP}_cabac${CABAC}_gop${GOP}${RC:+_rc$RC}

cd "$ROOT" || exit 1
"$HERE/cxx_enc"                        "$YUV" "$W" "$H" "$N" "$QP" "$CABAC" "$GOP" "$OUT/c_$TAG.264"  $RC 2>"$OUT/c_$TAG.log"
"$HERE/rust_enc/target/debug/rust_enc" "$YUV" "$W" "$H" "$N" "$QP" "$CABAC" "$GOP" "$OUT/r_$TAG.264" $RC 2>"$OUT/r_$TAG.log"

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
