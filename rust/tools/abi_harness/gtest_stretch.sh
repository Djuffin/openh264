#!/bin/bash
# Upstream's own `test/api` gtest suite, linked against the Rust cdylib — and against
# `libopenh264.a`, so the two tallies sit side by side (Phase 8 session C, T8.C5's
# stretch, time-boxed to an hour).
#
#   usage: rust/tools/abi_harness/gtest_stretch.sh
#
# **This is a number, not a gate.** It is not in `gates.sh`: 199 tests over ~35s that
# the port is known to fail 81 of would be a red gate on day one, and a gate nobody can
# make green is a gate that gets ignored. What it is good for is a *tally that moves* —
# Phase 9 can run it before and after and say what it bought.
#
# Three symbols the API objects need are internal to libopenh264 and are not among the
# seven a cdylib exports, so both binaries get them from the reference's own sources:
# `WelsSnprintf` (`crt_util_safe_x.cpp`) and `WelsCommon::g_keTypeMap` /
# `g_ksLevelLimits` (`common_tables.cpp`). They are data tables and a `vsnprintf`
# wrapper; neither is codec logic, and using the reference's copy on both links keeps
# the codec the only difference between them.
#
# Prerequisites: `make -j8 libraries binaries` at the repo root (for `test/api/*.o`,
# `libgtest.a` and `libopenh264.a`).
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
CRATE="$ROOT/rust/crates/openh264-rs"
OUT="$HERE/out/gtest"
mkdir -p "$OUT"
cd "$ROOT" || exit 1

for f in libgtest.a libopenh264.a test/api/decoder_test.o; do
  [ -f "$ROOT/$f" ] || { echo "missing $f — run 'make -j8 libraries binaries' at the repo root"; exit 2; }
done

echo "=== building the two glue objects"
c++ -std=c++11 -c -I "$ROOT/codec/common/inc" -I "$ROOT/codec/api/wels" \
    -o "$OUT/common_tables.o" "$ROOT/codec/common/src/common_tables.cpp"   || exit 1
c++ -std=c++11 -c -I "$ROOT/codec/common/inc" -I "$ROOT/codec/api/wels" \
    -o "$OUT/crt_util_safe_x.o" "$ROOT/codec/common/src/crt_util_safe_x.cpp" || exit 1

(cd "$CRATE" && cargo build --release --quiet) || exit 1
DYL="$CRATE/target/release/libopenh264_rs.dylib"
[ -f "$DYL" ] || DYL="$CRATE/target/release/libopenh264_rs.so"

echo "=== linking test/api against the Rust cdylib"
c++ -std=c++11 -o "$OUT/api_gtest_rust" test/api/*.o "$OUT/common_tables.o" "$OUT/crt_util_safe_x.o" \
    "$ROOT/libgtest.a" "$DYL" -lpthread -Wl,-rpath,"$(dirname "$DYL")" || exit 1
echo "=== linking test/api against libopenh264.a (the reference)"
c++ -std=c++11 -o "$OUT/api_gtest_cxx" test/api/*.o "$OUT/common_tables.o" "$OUT/crt_util_safe_x.o" \
    "$ROOT/libgtest.a" "$ROOT/libopenh264.a" -lpthread || exit 1

tally() {  # $1 = binary, $2 = label
  local log="$OUT/$2.log"
  "$1" > "$log" 2>&1
  local total pass fail
  total=$(grep -oE '^\[==========\] [0-9]+ tests? from .* ran' "$log" | grep -oE '[0-9]+' | head -1)
  pass=$(grep -oE '^\[  PASSED  \] [0-9]+ tests?' "$log" | grep -oE '[0-9]+' | head -1)
  fail=$(grep -oE '^\[  FAILED  \] [0-9]+ tests?, listed below' "$log" | grep -oE '[0-9]+' | head -1)
  printf '%-12s %s ran, %s passed, %s failed   (%s)\n' "$2" "${total:-?}" "${pass:-0}" "${fail:-0}" "$log"
}

echo
echo "=== test/api tallies"
tally "$OUT/api_gtest_cxx"  cxx
tally "$OUT/api_gtest_rust" rust

echo
echo "=== what the Rust link fails, by test (from the summary block)"
awk '/^\[  FAILED  \] [0-9]+ tests?, listed below:/{f=1;next} f' "$OUT/rust.log" \
  | grep '^\[  FAILED  \] ' | sed 's/^\[  FAILED  \] //;s/,.*//;s/ (.*//' \
  | sed 's#/[0-9]*$##' | sort | uniq -c | sort -rn
