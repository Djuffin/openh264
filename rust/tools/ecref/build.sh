#!/bin/bash
# Build `ecref` against the repo's own C++ decoder — the output-equivalence
# reference for damaged-input rows (Phase 5 session S, F43).
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)

c++ -std=c++11 -O2 \
  -I"$ROOT/codec/api/wels" \
  -o "$HERE/ecref" "$HERE/ecref.cpp" \
  "$ROOT/libopenh264.dylib" \
  -Wl,-rpath,"$ROOT"

echo "built $HERE/ecref"
