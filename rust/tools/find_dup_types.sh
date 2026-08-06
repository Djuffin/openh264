#!/bin/sh
# Gate 2 check: every encoder-internal type must be declared exactly once.
# Prints one line per type name that is declared in more than one encoder module.
# Silence == gate met.
#
# Usage: rust/tools/find_dup_types.sh [repo-root]
#
# Two passes:
#   1. exact-name duplicates
#   2. case-insensitive duplicates -- Phase 4 found `wels_preprocess::SWelsEncCtx`
#      shadowing the canonical `sWelsEncCtx`, two names differing only in the case of
#      the leading letter, which pass 1 reads as unrelated types. That one hid a
#      15-field fake context behind a `pub type sWelsEncCtx = SWelsEncCtx;` alias, so
#      every field access in the 2592-line preprocessor read the wrong offsets.
#
# Known remaining blind spots, each of which has hidden a real bug: type *aliases*
# (`pub type X = Y;`), `src/common/`, duplicated *functions*, and renames of the same
# layout to a different identifier (`SSpatialIndexMap` vs `SSpatialPicIndex`).

ROOT="${1:-$(dirname "$0")/../..}"
DIR="$ROOT/rust/crates/openh264-rs/src/encoder"

decls() {
    grep -Hn -E '^[[:space:]]*(pub )?(struct|enum|union) [A-Za-z_0-9]+' "$DIR"/*.rs \
    | sed -E 's#^.*/([a-z_0-9]+\.rs):([0-9]+):.*(struct|enum|union) ([A-Za-z_0-9]+).*#\4 \1:\2#'
}

# Pass 1: exact duplicates.
decls \
| sort \
| awk '{ n[$1]++; where[$1] = where[$1] " " $2 }
       END { for (t in n) if (n[t] > 1) printf "%-32s x%-3d%s\n", t, n[t], where[t] }' \
| sort

# Pass 2: names that collide only when case is ignored. Suppresses pairs already
# reported by pass 1 so a genuine exact duplicate is not printed twice.
decls \
| awk '{ key = tolower($1); n[key]++; names[key] = names[key] " " $1; where[key] = where[key] " " $2 }
       END {
           for (k in n) {
               if (n[k] < 2) continue
               split(names[k], a, " ")
               distinct = 0
               for (i in a) if (a[i] != a[1]) distinct = 1
               if (distinct) printf "%-32s x%-3d%s   [case-insensitive:%s]\n", a[1], n[k], where[k], names[k]
           }
       }' \
| sort
