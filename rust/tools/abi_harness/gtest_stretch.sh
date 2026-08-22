#!/bin/bash
# Upstream's own `test/api` gtest suite, linked against the Rust cdylib — and against
# `libopenh264.a`, so the two tallies sit side by side (Phase 8 session C, T8.C5's
# stretch; made a gate in Phase 8b session A, T8b.A1).
#
#   usage: rust/tools/abi_harness/gtest_stretch.sh [--check] [--filter=<gtest pattern>]
#                                                  [--seeds=<A>..<B>]
#
#     (no flags)      build + link both binaries, run both, print the two tallies and
#                     the Rust failures **by assertion site** (S47: a fixture name is
#                     a surface — 81 rows read by fixture name hid 30 decoder
#                     assertions for a day, which is what F82 cost).
#     --check         a **gate**: build + link the Rust binary only, run it, and
#                     compare its failures against `gtest_known_failures.txt`.
#                     rc 1 when a failing test is not in that list, when a listed
#                     test passes, or when a listed test did not run at all (each is
#                     a lie about coverage in a different direction). Prints
#                     `gtest: <pass>/<total>, allowlist <n>` as its verdict line —
#                     `gates.sh` requires both that line and rc 0.
#     --filter=<pat>  passed through as `--gtest_filter`. With a filter, the
#                     "listed test did not run" check is suppressed (the filter is
#                     why it did not run); the other two still apply, so a
#                     per-family run is a real red/green.
#     --seeds=A..B    **the finder, not a gate.** Runs the whole suite once per seed
#                     in [A,B] on *both* links and prints, per seed, the rows each
#                     link failed and — the reason it exists — the rows the Rust link
#                     failed that the C++ link did not. Roughly 80 s per seed (two
#                     full suites). Ignores `--check`.
#
# **S49 — the seed is pinned (Phase 8b session B, T8b.B1).** `test/api`'s own `main`
# (`test/api/simple_test.cpp:20-24`) seeds `rand()` from `time(NULL)` unless its first
# non-gtest argument is `--seed=N`; a dozen encoder tests build their input from
# `rand()`, so before this every run was a different suite and the ratchet's tally was
# a sample rather than a measurement. `EncodeDecodeTestAPI.SetOptionECIDC_SpecificFrameChange`
# was red **once in seven** unseeded runs at `5478ae7e` and green the other six (F88) —
# a gate that flips on its own is a gate that gets ignored. `SEED` below is passed to
# every run of both binaries, so `--check` is reproducible and the two links are
# compared on the *same* streams. Changing it re-samples the suite and will move rows:
# treat it as a baseline, and if it must move, move it in a commit that says why and
# re-owns whatever the new sample turns red.
#
# **Before T8b.A1 this was a number, not a gate**, because 199 tests the port failed
# 44 of would have been red on day one and a gate nobody can make green gets ignored.
# The allowlist is what makes it a ratchet: the tally now moves only by editing a
# named list, in the same commit as the fix that flips the row.
#
# Three symbols the API objects need are internal to libopenh264 and are not among the
# seven a cdylib exports, so both binaries get them from the reference's own sources:
# `WelsSnprintf` (`crt_util_safe_x.cpp`) and `WelsCommon::g_keTypeMap` /
# `g_ksLevelLimits` (`common_tables.cpp`). They are data tables and a `vsnprintf`
# wrapper; neither is codec logic, and using the reference's copy on both links keeps
# the codec the only difference between them.
#
# Prerequisites: `make -j8 libraries binaries` at the repo root (for `test/api/*.o`,
# `libgtest.a` and `libopenh264.a`).
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
CRATE="$ROOT/rust/crates/openh264-rs"
OUT="$HERE/out/gtest"
LIST="$HERE/gtest_known_failures.txt"
mkdir -p "$OUT"
cd "$ROOT" || exit 1

# The pinned suite seed (S49). One constant, both links, every mode. See the header.
SEED=20260822

CHECK=0
FILTER=""
SEEDS=""
for arg in "$@"; do
  case "$arg" in
    --check)    CHECK=1 ;;
    --filter=*) FILTER="${arg#--filter=}" ;;
    --seeds=*)  SEEDS="${arg#--seeds=}" ;;
    *) sed -n '2,46p' "$0"; exit 2 ;;
  esac
done

for f in libgtest.a libopenh264.a test/api/decoder_test.o; do
  [ -f "$ROOT/$f" ] || { echo "missing $f — run 'make -j8 libraries binaries' at the repo root"; exit 2; }
done

echo "=== building the two glue objects"
c++ -std=c++11 -c -I "$ROOT/codec/common/inc" -I "$ROOT/codec/api/wels" \
    -o "$OUT/common_tables.o" "$ROOT/codec/common/src/common_tables.cpp"   || exit 1
c++ -std=c++11 -c -I "$ROOT/codec/common/inc" -I "$ROOT/codec/api/wels" \
    -o "$OUT/crt_util_safe_x.o" "$ROOT/codec/common/src/crt_util_safe_x.cpp" || exit 1

(cd "$CRATE" && cargo build --release --quiet) || exit 1
DYL="$CRATE/target/release/libopenh264_rs.dylib"
[ -f "$DYL" ] || DYL="$CRATE/target/release/libopenh264_rs.so"

echo "=== linking test/api against the Rust cdylib"
c++ -std=c++11 -o "$OUT/api_gtest_rust" test/api/*.o "$OUT/common_tables.o" "$OUT/crt_util_safe_x.o" \
    "$ROOT/libgtest.a" "$DYL" -lpthread -Wl,-rpath,"$(dirname "$DYL")" || exit 1
# `--seeds` compares the two links, so it needs the reference binary even though it
# is not the `--check` path.
if [ "$CHECK" -eq 0 ] || [ -n "$SEEDS" ]; then
  echo "=== linking test/api against libopenh264.a (the reference)"
  c++ -std=c++11 -o "$OUT/api_gtest_cxx" test/api/*.o "$OUT/common_tables.o" "$OUT/crt_util_safe_x.o" \
      "$ROOT/libgtest.a" "$ROOT/libopenh264.a" -lpthread || exit 1
fi

# Every number below is parsed out of one of these logs; nothing is counted twice.
run_one() {  # $1 = binary, $2 = label, [$3 = seed]  -> writes $OUT/$2.log
  # `--seed=` first: `simple_test.cpp:21` reads it out of **argv[1]**, and it only
  # gets there because `InitGoogleTest` has already removed `--gtest_filter` from in
  # front of it. Putting it first means the read is right whether or not gtest
  # recognises whatever else is on the line.
  local args=("--seed=${3:-$SEED}")
  [ -n "$FILTER" ] && args+=("--gtest_filter=$FILTER")
  "$1" "${args[@]}" > "$OUT/$2.log" 2>&1
}

tally_line() {  # $1 = label
  local log="$OUT/$1.log" total pass fail
  total=$(grep -oE '^\[==========\] [0-9]+ tests? from .* ran' "$log" | grep -oE '[0-9]+' | head -1)
  pass=$(grep -oE '^\[  PASSED  \] [0-9]+ tests?' "$log" | grep -oE '[0-9]+' | head -1)
  fail=$(grep -oE '^\[  FAILED  \] [0-9]+ tests?, listed below' "$log" | grep -oE '[0-9]+' | head -1)
  printf '%-12s %s ran, %s passed, %s failed   (%s)\n' "$1" "${total:-?}" "${pass:-0}" "${fail:-0}" "$log"
}

# The *first* failing assertion site of every failed test, "<name>\t<file:line>".
# The `[  FAILED  ]` lines in gtest's closing summary are skipped because `t` has
# already been cleared by the per-test one that matched it.
sites() {  # $1 = log
  awk '
    /^\[ RUN      \]/ { t = $4; site = "" }
    /: Failure$/      { if (t != "" && site == "") { site = $1; sub(/:$/, "", site) } }
    /^\[  FAILED  \] / {
      n = $4; sub(/,$/, "", n)
      if (t != "" && n == t) { printf "%s\t%s\n", t, (site == "" ? "(no assertion line)" : site); t = "" }
    }
  ' "$1"
}

# Every test gtest actually ran, one name per line (`[ RUN      ]`).
ran_tests() { awk '/^\[ RUN      \]/ { print $4 }' "$1"; }

# The allowlist, name-only, comments and blanks dropped.
list_names() { grep -vE '^[[:space:]]*(#|$)' "$LIST" | sed 's/[[:space:]]*|.*//'; }

# ---------------------------------------------------------------------------
# --seeds=A..B: the finder. Not a gate — it prints evidence.
#
# A `--gtest_filter` changes the `rand()` call sequence, so a per-test repro of a
# seed-dependent failure does not exist: the seed pins the **suite**, and the row
# only fails inside the whole run. That is why this mode runs the full suite per
# seed rather than the family.
# ---------------------------------------------------------------------------
if [ -n "$SEEDS" ]; then
  lo=${SEEDS%%..*}; hi=${SEEDS##*..}
  case "$lo$hi" in *[!0-9]*|"") echo "--seeds wants A..B, both integers"; exit 2 ;; esac
  [ "$hi" -ge "$lo" ] || { echo "--seeds: A must be <= B"; exit 2; }
  work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
  # A `while` and not `seq`: BSD `seq` prints a seed of this magnitude as
  # `2.02608e+07`, which `atoi` then reads as **2** — every run would silently use
  # the same wrong seed. The `Random seed:` echo-back below is what caught it, and it
  # stays for the next such trap.
  seed=$lo
  while [ "$seed" -le "$hi" ]; do
    echo
    echo "=== seed $seed"
    run_one "$OUT/api_gtest_rust" "rust_$seed" "$seed"
    run_one "$OUT/api_gtest_cxx"  "cxx_$seed"  "$seed"
    for lbl in rust cxx; do
      grep -q "^Random seed: $seed\$" "$OUT/${lbl}_$seed.log" \
        || echo "  !! $lbl did not report 'Random seed: $seed' — the seed did not reach main()"
      tally_line "${lbl}_$seed"
      sites "$OUT/${lbl}_$seed.log" | cut -f1 | sort -u > "$work/$lbl"
    done
    echo "  --- rust failures the C++ link does not share (the port's, at this seed):"
    comm -23 "$work/rust" "$work/cxx" | sed 's/^/    /'
    echo "  --- failures both links share (upstream's, at this seed):"
    comm -12 "$work/rust" "$work/cxx" | wc -l | sed 's/^/    /;s/$/ rows/'
    seed=$((seed + 1))
  done
  exit 0
fi

if [ "$CHECK" -eq 0 ]; then
  echo
  echo "=== test/api tallies"
  run_one "$OUT/api_gtest_cxx"  cxx  ; tally_line cxx
  run_one "$OUT/api_gtest_rust" rust ; tally_line rust

  echo
  echo "=== what the Rust link fails, by assertion site (S47)"
  sites "$OUT/rust.log" | cut -f2 | sort | uniq -c | sort -rn
  exit 0
fi

# ---------------------------------------------------------------------------
# --check: the ratchet.
# ---------------------------------------------------------------------------
[ -f "$LIST" ] || { echo "gtest --check: missing $LIST"; exit 1; }
run_one "$OUT/api_gtest_rust" rust
tally_line rust

total=$(grep -oE '^\[==========\] [0-9]+ tests? from .* ran' "$OUT/rust.log" | grep -oE '[0-9]+' | head -1)
pass=$(grep -oE '^\[  PASSED  \] [0-9]+ tests?' "$OUT/rust.log" | grep -oE '[0-9]+' | head -1)
if [ -z "${total:-}" ]; then
  echo "gtest --check: the binary printed no '[==========] n tests ran' line — it died before finishing; see $OUT/rust.log"
  exit 1
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
sites "$OUT/rust.log" | cut -f1 | sort -u > "$tmp/failed"
ran_tests "$OUT/rust.log" | sort -u                > "$tmp/ran"
list_names | sort -u                               > "$tmp/listed"

rc=0

# 1. A failure nobody owns.
comm -23 "$tmp/failed" "$tmp/listed" > "$tmp/unlisted"
if [ -s "$tmp/unlisted" ]; then
  rc=1
  echo
  echo "gtest --check: FAILING BUT NOT IN $(basename "$LIST") — a regression, or a failure with no owner:"
  while IFS= read -r n; do
    printf '  %-72s %s\n' "$n" "$(sites "$OUT/rust.log" | awk -F'\t' -v n="$n" '$1==n{print $2}')"
  done < "$tmp/unlisted"
fi

# 2. A listed row that passes — the tally moved and the list did not.
comm -12 "$tmp/listed" "$tmp/ran" > "$tmp/listed_and_ran"
comm -23 "$tmp/listed_and_ran" "$tmp/failed" > "$tmp/stale"
if [ -s "$tmp/stale" ]; then
  rc=1
  echo
  echo "gtest --check: LISTED BUT PASSING — stale rows; delete them in the commit that fixed them:"
  sed 's/^/  /' "$tmp/stale"
fi

# 3. A listed row naming a test that did not run at all. Suppressed under --filter,
#    which is the ordinary reason for it.
if [ -z "$FILTER" ]; then
  comm -23 "$tmp/listed" "$tmp/ran" > "$tmp/never_ran"
  if [ -s "$tmp/never_ran" ]; then
    rc=1
    echo
    echo "gtest --check: LISTED BUT NEVER RAN — the name is misspelled or the test is gone:"
    sed 's/^/  /' "$tmp/never_ran"
  fi
fi

echo
echo "=== the Rust link's failures, by assertion site (S47)"
sites "$OUT/rust.log" | cut -f2 | sort | uniq -c | sort -rn

echo
# The verdict line `gates.sh` greps for. Only a completed comparison prints it.
printf 'gtest: %s/%s, allowlist %s\n' "${pass:-0}" "$total" "$(wc -l < "$tmp/listed" | tr -d ' ')"
exit "$rc"
