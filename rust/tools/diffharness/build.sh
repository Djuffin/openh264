#!/bin/bash
# Build both sides of the encoder differential harness.
# Requires `make -j8 libraries binaries` to have produced libopenh264.a first.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)

c++ -std=c++11 -I"$ROOT/codec/api/wels" -o "$HERE/cxx_enc" "$HERE/cxx_enc.cpp" \
    "$ROOT/libopenh264.a" -lpthread
echo "built $HERE/cxx_enc"

cd "$HERE/rust_enc" && cargo build --quiet
echo "built $HERE/rust_enc/target/debug/rust_enc"
