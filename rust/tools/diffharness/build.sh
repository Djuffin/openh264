#!/bin/bash
# Build both sides of the encoder differential harness.
# Requires `make -j8 libraries binaries` to have produced libopenh264.a first.
#
# RUST_ENC_PROFILE=debug (default) | release selects the Rust driver's build
# profile; compare.sh reads the same variable to pick the binary it runs. The
# gate battery wants both, because a release-only crash is how latent UB
# announced itself here once already (see the plan §1.4) — an optimised build
# is a second opinion on the same source, not a formality.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
PROFILE=${RUST_ENC_PROFILE:-debug}
case "$PROFILE" in
  debug|release) ;;
  *) echo "RUST_ENC_PROFILE must be debug or release, got: $PROFILE" >&2; exit 2 ;;
esac

c++ -std=c++11 -I"$ROOT/codec/api/wels" -o "$HERE/cxx_enc" "$HERE/cxx_enc.cpp" \
    "$ROOT/libopenh264.a" -lpthread
echo "built $HERE/cxx_enc"

# Spelled out per profile rather than accumulating flags in an array: `set -u`
# plus an *empty* array is an unbound-variable error in bash 3.2, which is what
# /bin/bash is on macOS, and the empty case is the default one.
cd "$HERE/rust_enc" || exit 1
if [ "$PROFILE" = release ]; then
  cargo build --quiet --release
else
  cargo build --quiet
fi
echo "built $HERE/rust_enc/target/$PROFILE/rust_enc"
