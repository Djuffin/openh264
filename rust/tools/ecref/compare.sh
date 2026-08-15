#!/bin/bash
# Compare the port's malformed-parity table against the C++ decoder, row by row.
#
#   usage: rust/tools/ecref/compare.sh <stream.264|-> <golden.txt> [ref]
#
#     ref  = a git revision to read the golden from instead of the worktree
#            (so "did F43 move the port toward the C++ or away from it?" is one
#            command per side).
#
#   env:
#     MALFORMED_DUMP_DIR=<dir>   replay EVERY row from the harness's corpus dump
#     SHOW_CODES=1               print the code mismatches, not just the tally
#
# **Every row gets a referee when the dump is present** (Phase 5 session U,
# T5.U2). Without it this compares `trunc.*` rows only, because those are the
# ones a `(file, length)` pair can name: the `tail.*`, `hdr*.*` and degenerate
# entries build their bytes inside the Rust harness. That left 389 of 2707 rows
# pinned against the port's own previous output — the blindness F43–F46 lived
# behind. Produce the dump with
#
#     MALFORMED_DUMP_DIR=/tmp/corpus cargo test --test malformed_stream_parity
#
# and every row replays through `ecref --stdin`, feed mode per the manifest.
#
# The dump is checked rather than trusted, which is what makes it evidence: the
# manifest's length must equal the table's `bytes` column on every row, and a
# `trunc.*`/`epb*` row's blob must be byte-identical to the base stream's prefix
# of that length — the two independent routes to the same bytes have to agree
# before either answer counts. A `-` base stream skips the prefix check (the
# degenerate table has no base stream).
#
# Prints one line per differing row and a tally. AGREE = frames, dims and the
# plane hash all match the C++; codes are tallied separately, because they are a
# separate verdict — F46 was 76 rows of one stream where every plane matched and
# the codes did not.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
STREAM=$1
GOLDEN=$2
REF=${3:-}
STEM=$(basename "$GOLDEN" .txt)
DUMP=${MALFORMED_DUMP_DIR:-}

export DYLD_LIBRARY_PATH="$ROOT"

if [ -n "$REF" ]; then
  TABLE=$(cd "$ROOT" && git show "$REF:rust/crates/openh264-rs/tests/data/malformed_parity/$(basename "$GOLDEN")")
else
  TABLE=$(cat "$ROOT/rust/crates/openh264-rs/tests/data/malformed_parity/$(basename "$GOLDEN")")
fi

# Feed mode and length per variant, from the dump's manifest, whose lines are
# `variant<TAB>feed<TAB>len`. Read per row with awk rather than into an
# associative array: /bin/bash here is 3.2, which has neither `declare -A` nor
# namerefs, and a manifest is at most 229 lines.
MANIFEST=""
if [ -n "$DUMP" ]; then
  MANIFEST="$DUMP/$STEM.manifest"
  [ -f "$MANIFEST" ] || { echo "no manifest at $MANIFEST" >&2; exit 2; }
fi
manifest_field() {  # $1 = variant, $2 = field index (2 = feed, 3 = len)
  awk -F'\t' -v v="$1" -v f="$2" '$1 == v { print $f; found=1; exit } END { if (!found) print "?" }' "$MANIFEST"
}

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

agree=0; differ=0; code_agree=0; code_differ=0; skipped=0; bad_dump=0
while read -r variant bytes calls drain frames dims sha ret_rle rest; do
  if [ -n "$DUMP" ]; then
    blob="$DUMP/$STEM/$variant.bin"
    if [ ! -f "$blob" ]; then
      printf 'NODUMP %-22s (no %s)\n' "$variant" "$blob"; bad_dump=$((bad_dump+1)); continue
    fi
    # The dump has to agree with the table it is refereeing, on every row.
    mlen=$(manifest_field "$variant" 3)
    if [ "$mlen" != "$bytes" ]; then
      printf 'DUMPLEN %-21s table=%s manifest=%s\n' "$variant" "$bytes" "$mlen"
      bad_dump=$((bad_dump+1)); continue
    fi
    # …and a prefix truncation must reach the same bytes both ways.
    case "$variant" in
      trunc.*|epb*)
        # `head -c 0` is an error on macOS, and `trunc.b000+00` is a real row
        # (the zero-length prefix), so the empty case reads from /dev/null.
        if [ "$STREAM" != "-" ] && \
           ! { [ "$bytes" -eq 0 ] && cat /dev/null || head -c "$bytes" "$ROOT/$STREAM"; } | cmp -s - "$blob"; then
          printf 'DUMPBYTES %-19s blob != %s[..%s]\n' "$variant" "$STREAM" "$bytes"
          bad_dump=$((bad_dump+1)); continue
        fi ;;
    esac
    feed=$(manifest_field "$variant" 2)
    out=$("$HERE/ecref" --stdin "--$feed" < "$blob")
  else
    case "$variant" in trunc.*) ;; *) skipped=$((skipped+1)); continue ;; esac
    out=$("$HERE/ecref" "$ROOT/$STREAM" "$bytes")
  fi

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

printf '\n%s%s: output %d agree / %d differ; codes %d agree / %d differ%s%s\n' \
  "$(basename "$GOLDEN")" "${REF:+ @$REF}" "$agree" "$differ" "$code_agree" "$code_differ" \
  "$([ "$skipped" -gt 0 ] && printf ' (%d non-trunc rows unrefereed — set MALFORMED_DUMP_DIR)' "$skipped")" \
  "$([ "$bad_dump" -gt 0 ] && printf ' [%d DUMP FAULTS]' "$bad_dump")"

[ "$bad_dump" -eq 0 ]
