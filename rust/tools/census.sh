#!/bin/bash
# The duplicate census, as a gate — plan §7.2, added Phase 5 session A.
#
#   usage: rust/tools/census.sh [report]
#
# Runs the three duplicate instruments and subtracts rust/tools/census_allowlist.txt.
# Exits non-zero if anything is reported that the allowlist does not cover, or if a
# counted budget is exceeded. `report` prints everything and always exits 0.
#
# Why a gate at all: the `transmute` metric went to zero in Phase 4b while
# `as *mut _ as *mut _` was doing the same job unwatched, and a struct declared twice
# is a divergence that has not happened yet (F21, F22). Names are what these
# instruments match on, so the allowlist keys on `<kind> <name> x<count>` — a *new*
# copy of an already-allowed name fails, which is the case that matters.
#
# Written as bash on purpose, like gates.sh and the diffharness: zsh does not
# word-split unquoted expansions.
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/.." && pwd)/..
ROOT=$(cd "$ROOT" && pwd)
SRC="$ROOT/rust/crates/openh264-rs/src"
ALLOW="$HERE/census_allowlist.txt"
MODE=${1:-check}

fails=0
note() { printf '%s\n' "$1"; }

# --- allowlist ------------------------------------------------------------
# Entries: strip comments, blank lines. Budgets are pulled out separately.
allowed() {  # $1 = "kind name xN"
  grep -v '^[[:space:]]*#' "$ALLOW" \
  | sed 's/#.*//' \
  | sed 's/[[:space:]]*$//' \
  | grep -qx "$1"
}
budget() {   # $1 = budget name -> prints the number, or 0
  grep -E "^budget[[:space:]]+$1[[:space:]]" "$ALLOW" | awk '{print $3}' | head -1
}

# --- 1. duplicate type / alias / table / const declarations ---------------
note "=== find_dup_types.sh"
unlisted=0
while read -r kind name count rest; do
  [ -z "${kind:-}" ] && continue
  case "$kind" in
    type|alias|table) key="$kind $name $count" ;;
    const) continue ;;   # value-divergent constants: reported, not gated (see below)
    *) continue ;;
  esac
  if allowed "$key"; then continue; fi
  printf '  UNLISTED  %-8s %-32s %-5s %s\n' "$kind" "$name" "$count" "$rest"
  unlisted=$((unlisted + 1))
done < <(sh "$HERE/find_dup_types.sh" "$ROOT" | grep -E '^(type|alias|table)')
if [ "$unlisted" -gt 0 ]; then
  note "  FAIL: $unlisted duplicate declaration(s) not in census_allowlist.txt"
  fails=$((fails + 1))
else
  note "  ok: every duplicate declaration is allowlisted"
fi

# --- 2. type laundering by double cast ------------------------------------
note "=== double casts"
inferred=$(grep -rn -e 'as \*mut _ as \*mut _' -e 'as \*const _ as \*const _' "$SRC" \
           | grep -vE ':[[:space:]]*//' | wc -l | tr -d ' ')
named=$(grep -rn -e 'as \*mut _ as \*mut ' -e 'as \*const _ as \*const ' "$SRC" \
        | grep -vE ':[[:space:]]*//' \
        | grep -vE 'as \*(mut|const) _ as \*(mut|const) _' | wc -l | tr -d ' ')
b_inf=$(budget inferred_double_casts); b_nam=$(budget named_double_casts)
printf '  inferred-target: %s (budget %s)   named-target: %s (budget %s)\n' \
       "$inferred" "$b_inf" "$named" "$b_nam"
if [ "$inferred" -gt "${b_inf:-0}" ]; then
  note "  FAIL: an inferred-target double cast reinterprets a type into itself or bridges"
  note "        two declarations of one entity. Delete it and let rustc type-check the"
  note "        argument; if it then fails to compile, you have found a duplicate."
  grep -rn -e 'as \*mut _ as \*mut _' -e 'as \*const _ as \*const _' "$SRC" \
    | grep -vE ':[[:space:]]*//' | sed "s#$SRC/#    #"
  fails=$((fails + 1))
fi
if [ "$named" -gt "${b_nam:-0}" ]; then
  note "  FAIL: named-target double casts rose above the allowlisted budget"
  fails=$((fails + 1))
fi

# --- 3. duplicate function bodies -----------------------------------------
note "=== find_stub_bodies.py --dups"
groups=$(python3 "$HERE/find_stub_bodies.py" --dups 2>/dev/null | grep -cE '^[A-Za-z_]')
b_grp=$(budget duplicate_fn_bodies)
printf '  duplicate-body groups: %s (budget %s)\n' "$groups" "$b_grp"
if [ "$groups" -gt "${b_grp:-0}" ]; then
  note "  FAIL: a new function name is now defined in more than one module."
  note "        Read the bodies side by side before assuming they agree — F21 and F22"
  note "        were both signature-identical copies that had drifted."
  fails=$((fails + 1))
fi

# --- 4. value-divergent constants: reported, never gated ------------------
# The instrument compares scalar `pub const`s by value and only reports names whose
# copies disagree. Every one of them is a deliberate per-consumer value today (S6);
# they are printed so a *new* one is visible in the log, not to fail the build.
divergent=$(sh "$HERE/find_dup_types.sh" "$ROOT" | grep -c '^const')
note "=== value-divergent constants: $divergent (reported, not gated — S6)"

if [ "$MODE" = report ]; then exit 0; fi
[ "$fails" -eq 0 ] && note "CENSUS: PASS" || note "CENSUS: FAIL ($fails check(s))"
exit "$fails"
