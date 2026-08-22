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

# --- and the same program against the **port** (Phase 8b session B, T8b.B5) -------
#
# `ecref` asks the C++ decoder a question; `ecref_rs` asks the port the *same*
# question through the same source, so any answer they give differently is the port's
# and nothing else. The cdylib exports the same seven symbols `ecref` links against,
# so this is one extra link line and no extra code — and it is how the port gets a
# referee for a decode configuration no in-tree test drives (`--ec=`, F88/F93).
#
# Skipped rather than fatal when the cdylib is not built: `compare.sh` and
# `compare_all.sh` only use `ecref`, and neither should start failing because a
# cargo profile is missing.
DYL="$ROOT/rust/crates/openh264-rs/target/release/libopenh264_rs.dylib"
[ -f "$DYL" ] || DYL="$ROOT/rust/crates/openh264-rs/target/debug/libopenh264_rs.dylib"
if [ -f "$DYL" ]; then
  c++ -std=c++11 -O2 \
    -I"$ROOT/codec/api/wels" \
    -o "$HERE/ecref_rs" "$HERE/ecref.cpp" \
    "$DYL" \
    -Wl,-rpath,"$(dirname "$DYL")"
  echo "built $HERE/ecref_rs (against $DYL)"
else
  echo "skipped ecref_rs: no cdylib at rust/crates/openh264-rs/target/{release,debug}"
fi
