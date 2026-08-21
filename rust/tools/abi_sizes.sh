#!/bin/bash
# Build and run `abi_sizes.c`, and refresh `abi_sizes.txt` — the committed record of
# the C++ side's own numbers for every type that crosses the ABI (T8.C4).
#
#   usage: rust/tools/abi_sizes.sh [--check]
#
#     (no flag)  rebuild, run, overwrite abi_sizes.txt
#     --check    rebuild, run, and fail if the output differs from abi_sizes.txt
#
# The file is compiled **twice** — once as C, once as C++ — and the two outputs are
# diffed for every type that exists in both. C is the record (it is the ABI a C
# caller sees, and the two `*Vtbl` structs exist only there); the C++ pass is the
# check that the numbers do not depend on which front end read the header, since the
# rest of this project's referees (`cxx_enc`, `ecref`, `abi_harness`) are all C++.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

cc  -std=c11   -I "$ROOT/codec/api/wels" -o "$TMP/abi_sizes_c"   "$HERE/abi_sizes.c"
c++ -std=c++11 -x c++ -I "$ROOT/codec/api/wels" -o "$TMP/abi_sizes_cxx" "$HERE/abi_sizes.c"

"$TMP/abi_sizes_c"   > "$TMP/out_c.txt"
"$TMP/abi_sizes_cxx" > "$TMP/out_cxx.txt"

# The C++ build has no `ISVC*Vtbl` (the header gives it abstract classes instead), so
# compare on the lines it does produce.
if ! diff <(grep -v 'ISVC.*Vtbl' "$TMP/out_c.txt") "$TMP/out_cxx.txt" > "$TMP/cxx.diff"; then
  echo "C and C++ disagree about the header's layout — that is a finding, not a flag to add:"
  cat "$TMP/cxx.diff"
  exit 1
fi
echo "C/C++ agree on $(grep -vc 'ISVC.*Vtbl' "$TMP/out_c.txt") lines"

{
  echo "# Sizes, alignments and offsets of every ABI-crossing type, dumped from"
  echo "# codec/api/wels/{codec_api,codec_app_def,codec_def}.h by rust/tools/abi_sizes.c."
  echo "# Regenerate with rust/tools/abi_sizes.sh; check with --check."
  echo "# host: $(uname -s) $(uname -m)   cc: $(cc --version 2>/dev/null | head -1)"
  echo "#"
  echo "# size <type> <sizeof> <alignof>"
  echo "# off  <type> <field> <offsetof>"
  cat "$TMP/out_c.txt"
} > "$TMP/abi_sizes.txt"

if [ "${1:-}" = --check ]; then
  # The provenance header carries the host and compiler version, which differ per
  # machine and are not the contract; the numbers are.
  if diff <(grep -v '^#' "$HERE/abi_sizes.txt") <(grep -v '^#' "$TMP/abi_sizes.txt"); then
    echo "abi_sizes.txt is current"
    exit 0
  fi
  echo "abi_sizes.txt is stale — the headers moved, or this host disagrees. Re-run without --check."
  exit 1
fi

cp "$TMP/abi_sizes.txt" "$HERE/abi_sizes.txt"
echo "wrote $HERE/abi_sizes.txt ($(grep -c '^size' "$HERE/abi_sizes.txt") types, $(grep -c '^off' "$HERE/abi_sizes.txt") field offsets)"
