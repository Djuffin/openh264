#!/bin/bash
# Build and run OpenH264's C++ interface test (`cpp_interface_test.cpp`) using
# GoogleTest (`gtest/googletest`), linked against the self-contained Rust `cxx`
# C++ bindings (`cxx_api.rs.h` / `cxx_api.rs.cc`) and `libopenh264_rs`.
#
#   usage: bash rust/tools/cpp_interface_test.sh

set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
CRATE="$ROOT/rust/crates/openh264-rs"
GTEST_DIR="$ROOT/gtest/googletest"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

if [ ! -d "$GTEST_DIR" ]; then
  echo "=== Bootstrapping GoogleTest via make gtest-bootstrap ==="
  make -C "$ROOT" gtest-bootstrap
fi

echo "=== Building libopenh264-rs (release cdylib + cxx bridge headers/sources) ==="
cargo build --release --manifest-path "$CRATE/Cargo.toml"

case "$(uname -s)" in
  Darwin) DYLIB="$CRATE/target/release/libopenh264_rs.dylib" ;;
  *)      DYLIB="$CRATE/target/release/libopenh264_rs.so" ;;
esac

CXX_HEADER_DIR="$CRATE/target/cxxbridge"
CXX_BRIDGE_CC="$CRATE/target/cxxbridge/openh264-rs/src/api/cxx_api.rs.cc"

echo "=== Compiling GoogleTest, cxx bridge C++ shim, and cpp_interface_test.cpp ==="
c++ -std=c++14 -I "$GTEST_DIR/include" -I "$GTEST_DIR" -pthread \
    -c "$GTEST_DIR/src/gtest-all.cc" -o "$TMP/gtest-all.o"
c++ -std=c++14 -I "$GTEST_DIR/include" -I "$GTEST_DIR" -pthread \
    -c "$GTEST_DIR/src/gtest_main.cc" -o "$TMP/gtest_main.o"

c++ -std=c++14 -I "$CXX_HEADER_DIR" \
    -c "$CXX_BRIDGE_CC" -o "$TMP/cxx_api.rs.o"

c++ -std=c++14 -I "$CXX_HEADER_DIR" -I "$GTEST_DIR/include" -pthread \
    -c "$HERE/cpp_interface_test.cpp" -o "$TMP/cpp_interface_test.o"

c++ -std=c++14 -pthread -o "$TMP/cpp_interface_test" \
    "$TMP/gtest-all.o" \
    "$TMP/gtest_main.o" \
    "$TMP/cxx_api.rs.o" \
    "$TMP/cpp_interface_test.o" \
    "$DYLIB" \
    -Wl,-rpath,"$(dirname "$DYLIB")"

echo "=== Running cpp_interface_test ==="
"$TMP/cpp_interface_test"
