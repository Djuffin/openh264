#!/bin/bash
# Compare the port's malformed-parity table against the C++ decoder, row by row.
#
#   usage: rust/tools/ecref/compare.sh <stream.264> <golden.txt> [ref]
#
#     ref  = a git revision to read the golden from instead of the worktree
#            (so "did F43 move the port toward the C++ or away from it?" is one
#            command per side).
#
# Only `trunc.*` rows are compared: they are pure prefix truncations, which is
# what `ecref` reproduces byte for byte. The other families (emulation-prevention
# edges, synthetic tails, raw feeds) build their bytes in the Rust harness and
# would need it to hand them over — stated here rather than left to be inferred.
#
# Prints one line per compared row and a tally. AGREE = frames, dims and the
# plane hash all match the C++.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
STREAM=$1
GOLDEN=$2
REF=${3:-}

export DYLD_LIBRARY_PATH="$ROOT"

if [ -n "$REF" ]; then
  TABLE=$(cd "$ROOT" && git show "$REF:rust/crates/openh264-rs/tests/data/malformed_parity/$(basename "$GOLDEN")")
else
  TABLE=$(cat "$ROOT/rust/crates/openh264-rs/tests/data/malformed_parity/$(basename "$GOLDEN")")
fi

agree=0; differ=0
while read -r variant bytes calls drain frames dims sha rest; do
  case "$variant" in trunc.*) ;; *) continue ;; esac
  out=$("$HERE/ecref" "$ROOT/$STREAM" "$bytes")
  c_frames=$(printf '%s' "$out" | cut -d' ' -f1)
  c_dims=$(printf '%s' "$out" | cut -d' ' -f2)
  c_sha=$(printf '%s' "$out" | cut -d' ' -f3)
  if [ "$c_frames" = "$frames" ] && [ "$c_dims" = "$dims" ] && [ "$c_sha" = "$sha" ]; then
    agree=$((agree+1))
  else
    differ=$((differ+1))
    printf 'DIFFER %-22s bytes=%-8s port=%s/%s/%s  cpp=%s/%s/%s\n' \
      "$variant" "$bytes" "$frames" "$dims" "${sha:0:12}" "$c_frames" "$c_dims" "${c_sha:0:12}"
  fi
done < <(printf '%s\n' "$TABLE" | grep -v '^#')

printf '\n%s%s: %d agree, %d differ (trunc rows only)\n' \
  "$(basename "$GOLDEN")" "${REF:+ @$REF}" "$agree" "$differ"
