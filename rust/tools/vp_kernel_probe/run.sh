#!/bin/bash
# Phase 8b session C, step 0 — the measurement this session pivots on.
#
# The tests link `libopenh264.a`, which is built with assembly enabled, so on this
# arm64 host `CDownsampling` dispatches to the **AArch64 NEON** downsamplers. The
# port would naturally translate the `_c` kernels. Two questions had to be answered
# by measurement before a single kernel was written:
#
#   1. `kernel_parity`  — are the `_c` kernels and their NEON siblings bit-identical?
#   2. `dispatch_model` — which arm of `CDownsampling::Process` actually runs, and
#                         does a faithful plain-C++ model of it reproduce the
#                         reference byte-for-byte on every ratio?
#
# Both link the reference archive and compare against it directly, so the answers
# are the library's, not a reading of the source. See `phase8b_findings.md` F97/F98.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)

CXXFLAGS="-std=c++11 -O2 -DHAVE_NEON_AARCH64"
INCS="-I$ROOT/codec/processing/src/downsample -I$ROOT/codec/processing/src/common \
      -I$ROOT/codec/processing/interface -I$ROOT/codec/common/inc -I$ROOT/codec/api/wels"

c++ $CXXFLAGS -o "$HERE/kernel_parity"  "$HERE/kernel_parity.cpp"  "$ROOT/libopenh264.a"
c++ $CXXFLAGS $INCS -o "$HERE/dispatch_model" "$HERE/dispatch_model.cpp" "$ROOT/libopenh264.a"

echo "=== 1. _c vs AArch64 NEON, kernel by kernel ==="
"$HERE/kernel_parity"
echo
echo "=== 2. the model of the arm that really runs, vs CDownsampling::Process ==="
"$HERE/dispatch_model"
