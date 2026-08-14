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

# Run-length encode a comma-separated code list the way the table's ret_rle does.
#
# **The `""` concatenations are load-bearing.** These records are `0x0`, `0x24`,
# … and macOS's awk (BWK, 20200816) reads `0x0` as a *hex numeral*, so a bare
# `$0 == prev` against an uninitialised `prev` is `0 == 0` — true on the very
# first record, after which `prev` is never assigned and every list RLEs to
# `*<n>`. It reported 194 of 200 rows as code mismatches on a stream whose real
# count is 76. Concatenating `""` forces the string comparison this wants.
rle() {
  printf '%s\n' "$1" | tr ',' '\n' | awk '
    { if (($0 "") == (prev "")) { n++ }
      else { if (NR>1) printf "%s,", (n>1 ? (prev "*" n) : prev); prev=$0; n=1 } }
    END { printf "%s\n", (n>1 ? (prev "*" n) : prev) }'
}

agree=0; differ=0; code_agree=0; code_differ=0
while read -r variant bytes calls drain frames dims sha ret_rle rest; do
  case "$variant" in trunc.*) ;; *) continue ;; esac
  out=$("$HERE/ecref" "$ROOT/$STREAM" "$bytes")
  c_frames=$(printf '%s' "$out" | cut -d' ' -f1)
  c_dims=$(printf '%s' "$out" | cut -d' ' -f2)
  c_sha=$(printf '%s' "$out" | cut -d' ' -f3)
  c_ret=$(rle "$(printf '%s' "$out" | cut -d' ' -f4)")

  if [ "$c_frames" = "$frames" ] && [ "$c_dims" = "$dims" ] && [ "$c_sha" = "$sha" ]; then
    agree=$((agree+1))
  else
    differ=$((differ+1))
    printf 'DIFFER %-22s bytes=%-8s port=%s/%s/%s  cpp=%s/%s/%s\n' \
      "$variant" "$bytes" "$frames" "$dims" "${sha:0:12}" "$c_frames" "$c_dims" "${c_sha:0:12}"
  fi

  # The DECODING_STATE the API hands back is observable behaviour too — callers
  # branch on it — and it is a *separate* verdict from the pixels: F46 is 76 rows
  # of one stream where every plane matches and the codes do not.
  if [ "$c_ret" = "$ret_rle" ]; then
    code_agree=$((code_agree+1))
  else
    code_differ=$((code_differ+1))
    if [ -n "${SHOW_CODES:-}" ]; then
      printf 'CODES  %-22s bytes=%-8s port=%s  cpp=%s\n' "$variant" "$bytes" "$ret_rle" "$c_ret"
    fi
  fi
done < <(printf '%s\n' "$TABLE" | grep -v '^#')

printf '\n%s%s: output %d agree / %d differ; codes %d agree / %d differ (trunc rows only)\n' \
  "$(basename "$GOLDEN")" "${REF:+ @$REF}" "$agree" "$differ" "$code_agree" "$code_differ"
