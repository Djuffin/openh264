#!/bin/bash
# The unsafe ratchet — plan §7.1.
#
#   usage: rust/tools/unsafe_ratchet.sh [generate | check | report]
#
#     generate   recount and (over)write rust/tools/unsafe_baseline.json
#     check      recount and fail if any file x metric INCREASED vs the baseline
#     report     print the current totals and the delta vs the baseline, exit 0
#
# Why this exists rather than `grep -c unsafe`: `src/lib.rs` sets
# `#![allow(unsafe_op_in_unsafe_fn)]`, so inside the port's ~1,400 `unsafe fn`
# bodies every deref and every `.add()` is unguarded and invisible to `unsafe {`
# counting. The real unit of progress is "unsafe fn bodies converted", which only
# a per-file, per-metric count can see.
#
# Counting rules, and the two traps that already cost this project a measurement:
#
#   1. `unsafe fn` alone does NOT match `unsafe extern "C" fn`. Phase 0's T5b
#      deleted 113 definitions and the naive pattern reported no change at all
#      (see the session log). The pattern here is `unsafe (extern "C" )?fn`.
#   2. That same pattern matches *function-pointer types* as well as definitions
#      — `Option<unsafe extern "C" fn(..)>` occurs ~210 times in the dispatch
#      tables. Definitions are therefore counted line-anchored (`unsafe_fn`) and
#      the pointer types are left to the `*mut`/`*const` and `transmute` metrics,
#      which is where Phase 4 will show its work.
#
# Decreases are always fine. `check` is a ratchet, not an equality test.
#
#   3. **Comments are stripped before counting (F178, session J).** `raw_ptr` and
#      `no_mangle` are occurrence counts, and the port's comments quote the code
#      they explain — `// a raw derived from *mut SDqLayer` counted as a pointer,
#      and twice a session failed its gate by *documenting* a conversion. The
#      `sed` below drops `//` to end-of-line. It cannot see a `//` inside a
#      string literal (it would truncate the rest of that line); measured over
#      this tree at the fix's commit, no counted token follows a string-borne
#      `//` on any line, and the calibration diff listed every file the strip
#      changed. Block comments are not stripped: the tree has no `/* */` bodies
#      carrying counted tokens (grep at the same commit).
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
SRC="$ROOT/rust/crates/openh264-rs/src"
BASELINE="$HERE/unsafe_baseline.json"

# metric name : extended regex : mode
#   line = count matching lines, anchored at the start of a line (definitions)
#   occ  = count occurrences (grep -o), because one line can hold several
METRICS=(
  'unsafe_fn|^[[:space:]]*(pub(\([a-z]+\))? )?unsafe (extern "C" )?fn |line'
  'unsafe_block|unsafe \{|occ'
  'raw_ptr|\*mut |\*const |occ'
  'transmute|transmute|occ'
  'unsafe_impl|unsafe impl|occ'
  'mem_zeroed|mem::zeroed|occ'
  'no_mangle|no_mangle|occ'
  'shim|SHIM\(|occ'
)

metric_names() {
  local spec
  for spec in "${METRICS[@]}"; do printf '%s\n' "${spec%%|*}"; done
}

# F178: comment text is not code. Strip `//` to end-of-line before counting.
strip_line_comments() { sed -E 's,//.*$,,' "$1"; }

# Emit "<relative path>\t<metric>\t<count>" for every .rs file under src/.
count_all() {
  local f rel spec name pat mode n
  # `find | sort` rather than a glob: stable ordering across machines is what
  # makes the baseline diff cleanly.
  while IFS= read -r f; do
    rel=${f#"$SRC"/}
    for spec in "${METRICS[@]}"; do
      name=${spec%%|*}
      mode=${spec##*|}
      pat=${spec#*|}; pat=${pat%|*}
      if [ "$mode" = line ]; then
        n=$(strip_line_comments "$f" | grep -Ec "$pat")
      else
        n=$(strip_line_comments "$f" | grep -Eo "$pat" | wc -l)
      fi
      printf '%s\t%s\t%s\n' "$rel" "$name" "$((n))"
    done
  done < <(find "$SRC" -name '*.rs' | sort)
}

# The baseline is JSON with exactly one line per file, so this awk is a parser
# and not a hopeful regex: the writer below is the only thing that produces it.
baseline_tsv() {
  [ -f "$BASELINE" ] || return 1
  awk '
    /^    "/ {
      file = $0
      sub(/^    "/, "", file); sub(/".*/, "", file)
      rest = $0
      while (match(rest, /"[a-z_]+": [0-9]+/)) {
        kv = substr(rest, RSTART, RLENGTH)
        rest = substr(rest, RSTART + RLENGTH)
        split(kv, a, "\": ")
        key = a[1]; sub(/^"/, "", key)
        if (key != file) printf "%s\t%s\t%s\n", file, key, a[2]
      }
    }
  ' "$BASELINE"
}

write_baseline() {
  local commit
  commit=$(cd "$ROOT" && git rev-parse --short HEAD 2>/dev/null || echo unknown)
  {
    echo '{'
    printf '  "commit": "%s",\n' "$commit"
    printf '  "metrics": ['
    metric_names | awk '{printf "%s\"%s\"", (NR>1 ? ", " : ""), $0}'
    printf '],\n'
    echo '  "files": {'
    count_all | awk -F'\t' '
      { if ($1 != cur) { if (cur != "") print line " },"; cur = $1
                         line = sprintf("    \"%s\": {", $1); first = 1 }
        line = line sprintf("%s \"%s\": %s", (first ? "" : ","), $2, $3); first = 0 }
      END { if (cur != "") print line " }" }
    '
    echo '  }'
    echo '}'
  } > "$BASELINE.tmp"
  mv "$BASELINE.tmp" "$BASELINE"
  echo "wrote $BASELINE (at $commit)"
}

totals() {  # reads TSV on stdin
  awk -F'\t' '{ t[$2] += $3 } END { for (m in t) printf "%s\t%s\n", m, t[m] }' | sort
}

case "${1:-check}" in
  generate)
    write_baseline
    echo
    echo "totals:"
    count_all | totals | awk -F'\t' '{ printf "  %-14s %s\n", $1, $2 }'
    ;;

  report|check)
    mode=${1:-check}
    cur=$(count_all)
    if ! base=$(baseline_tsv); then
      echo "no baseline at $BASELINE — run: $0 generate" >&2
      exit 2
    fi

    echo "metric          baseline      now     delta"
    join -t"$(printf '\t')" \
      <(printf '%s\n' "$base" | totals) \
      <(printf '%s\n' "$cur" | totals) |
      awk -F'\t' '{ printf "%-14s %8s %8s %+9d\n", $1, $2, $3, $3 - $2 }'

    # Per-file increases. A file absent from the baseline is compared against 0,
    # which is what makes a brand-new unsafe module fail the gate rather than
    # sneak in under a shrinking total.
    inc=$(join -t"$(printf '\t')" -a2 -e0 -o 0,1.2,2.2 \
            <(printf '%s\n' "$base" | awk -F'\t' '{printf "%s\x01%s\t%s\n", $1, $2, $3}' | sort) \
            <(printf '%s\n' "$cur"  | awk -F'\t' '{printf "%s\x01%s\t%s\n", $1, $2, $3}' | sort) |
          awk -F'\t' '$3 > $2 { split($1, k, "\x01"); printf "  %s  %s: %s -> %s\n", k[1], k[2], $2, $3 }')

    if [ -n "$inc" ]; then
      echo
      echo "INCREASES vs baseline:"
      printf '%s\n' "$inc"
      [ "$mode" = check ] && exit 1
    else
      echo
      echo "no per-file increases vs baseline."
    fi
    ;;

  *)
    sed -n '2,10p' "$0"
    exit 2
    ;;
esac
