#!/bin/sh
# Gate 2 check: every encoder-internal type must be declared exactly once.
# Prints one line per type name that is declared in more than one encoder module.
# Silence == gate met.
#
# Usage: rust/tools/find_dup_types.sh [repo-root]

ROOT="${1:-$(dirname "$0")/../..}"
DIR="$ROOT/rust/crates/openh264-rs/src/encoder"

grep -Hn -E '^[[:space:]]*(pub )?(struct|enum|union) [A-Za-z_0-9]+' "$DIR"/*.rs \
| sed -E 's#^.*/([a-z_0-9]+\.rs):([0-9]+):.*(struct|enum|union) ([A-Za-z_0-9]+).*#\4 \1:\2#' \
| sort \
| awk '{ n[$1]++; where[$1] = where[$1] " " $2 } END { for (t in n) if (n[t] > 1) printf "%-32s x%-3d%s\n", t, n[t], where[t] }' \
| sort
