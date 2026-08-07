#!/bin/sh
# Gate 2 check: every declaration must exist exactly once.
# Prints one line per name declared in more than one module. Silence == gate met.
#
# Usage: rust/tools/find_dup_types.sh [repo-root]
#
# Scope: `src/encoder`, `src/common`, `src/decoder` and `src/processing`. It used
# to read `src/encoder` only, and both of the other directories had already hidden
# real bugs (`SBitStringAux` x4; `PSampleSadSatdCostFunc` x5, one with the wrong
# constness; `g_kuiGolombUELength` x3, two with wrong values, one short enough to
# index out of bounds; `EWelsSliceType` twice in the decoder).
#
# What it checks:
#   1. types      -- struct / enum / union, exact-name duplicates
#   2. types      -- case-insensitive duplicates. Phase 4 found
#                    `wels_preprocess::SWelsEncCtx` shadowing the canonical
#                    `sWelsEncCtx`, two names differing only in the case of the
#                    leading letter, which pass 1 reads as unrelated types. That one
#                    hid a 15-field fake context behind a `pub type sWelsEncCtx =
#                    SWelsEncCtx;` alias, so every field access in the 2592-line
#                    preprocessor read the wrong offsets.
#   3. aliases    -- `pub type X = ...;`
#   4. tables     -- `pub const NAME: [...]` and `pub static NAME: [...]`. A
#                    duplicated table is worse than a duplicated scalar, because only
#                    an element-by-element diff shows the disagreement.
#   5. constants  -- scalar `pub const`, **compared by value**. A count of names
#                    proves nothing: seven phases running, a duplicated constant has
#                    held different values in different modules (GOM_SAD/GOM_VAR,
#                    g_kuiGolombUELength, DELTA_QP, MAX_FRAME_RATE, MAX_PPS_COUNT,
#                    MAX_SLICES_NUM_TMP, WELS_CPU_NEON with seven distinct values
#                    across eight modules). Only differing values are reported.
#
# `pub use` re-exports are deliberately not counted: re-exporting one definition is
# the fix this script asks for, not a new declaration.
#
# Remaining blind spots, both needing a human:
#   - a duplicated *function body* under the same name. `find_stub_bodies.py --dups`
#     lists those, worst body-size disparity first.
#   - the same layout under a *different* name (`SSpatialIndexMap` vs
#     `SSpatialPicIndex`). No identifier comparison can see it; only the header can.

ROOT="${1:-$(dirname "$0")/../..}"
SRC="$ROOT/rust/crates/openh264-rs/src"
DIRS="$SRC/encoder $SRC/common $SRC/decoder $SRC/processing"

files() {
    for d in $DIRS; do
        [ -d "$d" ] && ls "$d"/*.rs 2>/dev/null
    done
}

decls() {
    grep -Hn -E '^[[:space:]]*(pub )?(struct|enum|union) [A-Za-z_0-9]+' $(files) \
    | sed -E 's#^.*/([a-z_0-9]+\.rs):([0-9]+):.*(struct|enum|union) ([A-Za-z_0-9]+).*#\4 \1:\2#'
}

aliases() {
    grep -Hn -E '^[[:space:]]*pub type [A-Za-z_0-9]+[[:space:]]*=' $(files) \
    | sed -E 's#^.*/([a-z_0-9]+\.rs):([0-9]+):.*pub type ([A-Za-z_0-9]+).*#\3 \1:\2#'
}

tables() {
    grep -Hn -E '^[[:space:]]*pub (const|static) [A-Za-z_0-9]+[[:space:]]*:[[:space:]]*\[' $(files) \
    | sed -E 's#^.*/([a-z_0-9]+\.rs):([0-9]+):[[:space:]]*pub (const|static) ([A-Za-z_0-9]+).*#\4 \1:\2#'
}

dup_report() {
    label="$1"
    sort \
    | awk -v label="$label" \
          '{ n[$1]++; where[$1] = where[$1] " " $2 }
           END { for (t in n) if (n[t] > 1) printf "%-10s %-32s x%-3d%s\n", label, t, n[t], where[t] }' \
    | sort -k2
}

decls   | dup_report "type"
aliases | dup_report "alias"
tables  | dup_report "table"

# Case-insensitive type collisions. Suppresses pairs already reported above so a
# genuine exact duplicate is not printed twice.
decls \
| awk '{ key = tolower($1); n[key]++; names[key] = names[key] " " $1; where[key] = where[key] " " $2 }
       END {
           for (k in n) {
               if (n[k] < 2) continue
               split(names[k], a, " ")
               distinct = 0
               for (i in a) if (a[i] != a[1]) distinct = 1
               if (distinct) printf "%-10s %-32s x%-3d%s   [case-insensitive:%s]\n", "type", a[1], n[k], where[k], names[k]
           }
       }' \
| sort -k2

# Scalar constants, compared by VALUE. Names that agree everywhere are not a defect.
grep -Hn -E '^[[:space:]]*pub const [A-Za-z_0-9]+[[:space:]]*:[[:space:]]*[^[][^=]*=' $(files) \
| sed -E 's#^.*/([a-z_0-9]+\.rs):([0-9]+):[[:space:]]*pub const ([A-Za-z_0-9]+)[[:space:]]*:[^=]*=[[:space:]]*(.*);[[:space:]]*(//.*)?$#\3\t\4\t\1:\2#' \
| sort \
| awk -F'\t' '
    { n[$1]++; if (!($1 SUBSEP $2 in seen)) { seen[$1 SUBSEP $2] = 1; v[$1]++ }
      where[$1] = where[$1] sprintf("\n               %-40s %s", $2, $3) }
    END { for (c in n) if (n[c] > 1 && v[c] > 1)
            printf "%-10s %-32s x%-3d (%d distinct values)%s\n", "const", c, n[c], v[c], where[c] }' \
| sort -k2
