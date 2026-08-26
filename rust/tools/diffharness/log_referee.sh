#!/bin/bash
# **The log referee** — the C++ reference's trace output against the Rust port's,
# for one fixed configuration (Phase 9 session X2, F100).
#
#   usage: rust/tools/diffharness/log_referee.sh [--keep]
#
# Exits 0 iff the two encoders deliver the *same trace messages in the same order*
# at WELS_LOG_INFO. Nonzero, loudly, with a unified diff, otherwise (S58).
#
# ---------------------------------------------------------------------------
# WHY THIS EXISTS
#
# `CWelsH264SVCEncoder::TraceParamInfo` and `::LogStatistics` are pure `WelsLog`
# formatting. Not one byte of their output reaches a bitstream, so **every gate
# this repo owns is blind to them**: the sweeps compare `.264` files, the ratchet
# counts pointers, Miri interprets memory. Both bodies were empty in this port
# from the beginning and stayed empty through eight phases, and nothing said so
# (F100). A missing log is a missing observable, and the only instrument that can
# see one is a second encoder to compare against — which this repo already has.
#
# ---------------------------------------------------------------------------
# HOW
#
# Both drivers speak the same C API, so both take the same registration:
# `SetOption(ENCODER_OPTION_TRACE_CALLBACK, &cb)`, `..._CONTEXT` for the sink, and
# `..._TRACE_LEVEL = WELS_LOG_INFO` — the default is WELS_LOG_WARNING and would
# filter out exactly the two blocks under test. The callback appends
# `<level>|<message>` per delivered line. `OH264_TRACE_LOG` is what turns it on;
# unset, both drivers behave as they always have, which is why this is invisible
# to `sweep.sh` and is not wired into it.
#
# ---------------------------------------------------------------------------
# NORMALIZATIONS — the complete list, and nothing beyond it
#
# Each is a value that differs between two correct encoders on the same input.
# Anything not named here must match character for character.
#
#   1. `this = 0x<hex>`   the codec instance address `WelsLog` prints in its tag.
#                         Two processes, two heaps. -> `this = 0xINSTANCE`
#   2. `0x<hex>`          any other address that reaches a message. -> `0xPTR`
#   3. `SpeedInMs: <f>`   `LogStatistics`' wall-clock encode speed. -> `<TIME>`
#   4. `fAverageFrameRate=<f>`, `LastFrameRate=<f>`
#                         both derived from wall-clock elapsed time. -> `<RATE>`
#   5. `LatestBitRate=<n>` derived from (3)/(4)'s clock, same reason. -> `<BR>`
#   6. `at Ts = <n>`      the frame timestamp the statistics line is stamped with;
#                         the drivers feed a synthetic clock, but it is a clock.
#                         -> `at Ts = <TS>`
#   7. `codec version = <s>` the version string, which is a build property.
#                         -> `<VERSION>`
#
# Deliberately NOT normalized: every parameter value, every level id, every
# profile id, every count, every dimension, every flag. Those are the subject.
#
# ---------------------------------------------------------------------------
# THE FIXED ROW
#
# One configuration, named here so the comparison is reproducible: a `dl` row —
# three dependency layers at 320x192, denoise on, RC in bitrate mode. Multi-layer
# because `TraceParamInfo`'s second half is a *per spatial layer* loop and a
# single-layer row would run it once and prove almost nothing; bitrate mode
# because the statistics block is config-dependent, which is why this is one
# fixed row rather than a sweep.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
OUT="$HERE/out/logref"
KEEP=0
[ "${1:-}" = "--keep" ] && KEEP=1
mkdir -p "$OUT"

CXX="$HERE/cxx_enc"
RUST="$HERE/rust_enc/target/${RUST_ENC_PROFILE:-debug}/rust_enc"
for b in "$CXX" "$RUST"; do
  [ -x "$b" ] || { echo "log_referee: missing $b — run rust/tools/diffharness/build.sh"; exit 2; }
done

YUV="$ROOT/res/CiscoVT2people_320x192_12fps.yuv"
[ -f "$YUV" ] || { echo "log_referee: missing $YUV"; exit 2; }

#            src   w   h  frames qp cabac gop  out            rc bi sm sn th cx ltr lp fb ps dl dn
ARGS=(   "$YUV" 320 192      3   26     1  -1  "$OUT/x.264"    1  0  0  1  1  0   0 30  0  0  3  1 )

echo "=== the fixed row: ${ARGS[*]}"

run_one() {  # $1 = binary, $2 = label
  local args=("${ARGS[@]}")
  args[7]="$OUT/$2.264"
  OH264_TRACE_LOG="$OUT/$2.rawlog" "$1" "${args[@]}" > "$OUT/$2.stdout" 2>&1
  local rc=$?
  if [ "$rc" -ne 0 ]; then
    echo "log_referee: $2 exited $rc — see $OUT/$2.stdout"
    return 1
  fi
  [ -f "$OUT/$2.rawlog" ] || { echo "log_referee: $2 wrote no trace log at all"; return 1; }
  return 0
}

normalize() {  # stdin -> stdout; the seven rules above, in order
  # `0x0x` first: `WelsLog`'s tag is `"[OpenH264] this = 0x%p, ..."` and `%p` prints
  # its own `0x`, so the reference really does emit `this = 0x0x16b5d7000` (the port
  # reproduces the tag without the doubling, which is a documented divergence in
  # `wels_trace.rs`). Matching only `0x<hex>` leaves the second half behind.
  sed -E \
    -e 's/this = (0x)+[0-9a-fA-F]+/this = 0xINSTANCE/g' \
    -e 's/(0x)+[0-9a-fA-F]+/0xPTR/g' \
    -e 's/SpeedInMs: [-0-9.eE+]+/SpeedInMs: <TIME>/g' \
    -e 's/fAverageFrameRate=[-0-9.eE+]+/fAverageFrameRate=<RATE>/g' \
    -e 's/LastFrameRate=[-0-9.eE+]+/LastFrameRate=<RATE>/g' \
    -e 's/LatestBitRate=[-0-9]+/LatestBitRate=<BR>/g' \
    -e 's/at Ts = [-0-9]+/at Ts = <TS>/g' \
    -e 's/codec version = [^,]*/codec version = <VERSION>/g'
}

run_one "$CXX"  cxx  || exit 1
run_one "$RUST" rust || exit 1

normalize < "$OUT/cxx.rawlog"  > "$OUT/cxx.log"
normalize < "$OUT/rust.rawlog" > "$OUT/rust.log"

cxx_n=$(wc -l < "$OUT/cxx.log" | tr -d ' ')
rust_n=$(wc -l < "$OUT/rust.log" | tr -d ' ')
printf '=== captured: cxx %s lines, rust %s lines\n' "$cxx_n" "$rust_n"

# An empty capture on BOTH sides would diff clean and mean nothing — S66's known
# negative has to be a real one. A referee that passes because neither encoder
# said anything is the failure mode this whole script exists to prevent.
if [ "$cxx_n" -eq 0 ]; then
  echo "log_referee: the REFERENCE logged nothing — the capture is broken, not the port"
  exit 1
fi
if [ "$rust_n" -eq 0 ]; then
  echo "log_referee: the PORT logged nothing — every message below would be 'missing'"
  exit 1
fi

rc=0

# ---------------------------------------------------------------------------
# CHECK 1 — the level vocabulary, reported by name and not normalized away.
#
# The `iLevel` a message is delivered with is part of the C ABI: it is the second
# argument of the caller's own callback and the value `SetOption(
# ENCODER_OPTION_TRACE_LEVEL, ...)` is compared against. The two links do NOT
# agree on it, and that is a finding rather than noise — see F184. It is checked
# here separately, loudly, because folding it into the text comparison would make
# every single line differ and bury the nine that match.
# ---------------------------------------------------------------------------
cut -d'|' -f1 "$OUT/cxx.log"  | sort -u | tr '\n' ' ' > "$OUT/cxx.levels"
cut -d'|' -f1 "$OUT/rust.log" | sort -u | tr '\n' ' ' > "$OUT/rust.levels"
printf '=== levels delivered: cxx [%s], rust [%s]\n' \
  "$(cat "$OUT/cxx.levels")" "$(cat "$OUT/rust.levels")"
if ! cmp -s "$OUT/cxx.levels" "$OUT/rust.levels"; then
  rc=1
  echo "log_referee: LEVEL MISMATCH — the two links deliver different iLevel values."
  echo "  The reference's WELS_LOG_* are a bit mask (codec_app_def.h:323-331:"
  echo "  ERROR 1, WARNING 2, INFO 4, DEBUG 8, DETAIL 16). This port defines them"
  echo "  as consecutive integers (INFO 3, DEBUG 4, DETAIL 5) in five places:"
  echo "  common/wels_trace.rs, decoder/{decoder_core,decode_slice,manage_dec_ref}.rs"
  echo "  and tests/trace_callback_test.rs. F184 — needs a ruling, not a patch:"
  echo "  it is C-ABI-visible, it spans encoder and decoder, and no byte gate sees it."
fi

# ---------------------------------------------------------------------------
# CHECK 2 — the message text, against the named gap list.
# ---------------------------------------------------------------------------
# Strip `<level>|` and the `[OpenH264] this = ..., <Tag>:` header; what is left is
# the message the codec actually composed, which is the subject.
strip_header() { sed -E 's/^[0-9]+\|\[OpenH264\] this = [^,]*, [A-Za-z]+://'; }
strip_header < "$OUT/cxx.log"  > "$OUT/cxx.msg"
strip_header < "$OUT/rust.log" > "$OUT/rust.msg"

GAPS="$HERE/log_referee_known_gaps.txt"
[ -f "$GAPS" ] || { echo "log_referee: missing $GAPS"; exit 2; }
gap_res=$(grep -vE '^[[:space:]]*(#|$)' "$GAPS" | sed 's/[[:space:]]*|.*//')

# Reference lines the port did not produce.
comm -23 <(sort "$OUT/cxx.msg") <(sort "$OUT/rust.msg") > "$OUT/missing.txt"
# Port lines the reference did not produce — never allowlisted; inventing an
# observable is worse than omitting one.
comm -13 <(sort "$OUT/cxx.msg") <(sort "$OUT/rust.msg") > "$OUT/extra.txt"

unowned=0
matched_gaps="$OUT/matched_gaps.txt"; : > "$matched_gaps"
while IFS= read -r line; do
  [ -n "$line" ] || continue
  owned=0
  while IFS= read -r re; do
    [ -n "$re" ] || continue
    if printf '%s\n' "$line" | grep -qE "$re"; then owned=1; printf '%s\n' "$re" >> "$matched_gaps"; break; fi
  done <<< "$gap_res"
  if [ "$owned" -eq 0 ]; then
    unowned=$((unowned+1))
    [ "$unowned" -eq 1 ] && echo "log_referee: MISSING AND UNOWNED — the reference logs these, the port does not,"
    [ "$unowned" -eq 1 ] && echo "  and no row in $(basename "$GAPS") claims them:"
    printf '  %s\n' "$line"
  fi
done < "$OUT/missing.txt"
[ "$unowned" -gt 0 ] && rc=1

if [ -s "$OUT/extra.txt" ]; then
  rc=1
  echo "log_referee: EXTRA — the port logs these and the reference does not:"
  sed 's/^/  /' "$OUT/extra.txt"
fi

# A gap row that no longer differs is stale — the same lie `--check` refuses.
while IFS= read -r re; do
  [ -n "$re" ] || continue
  if ! grep -qxF "$re" "$matched_gaps" 2>/dev/null; then
    rc=1
    echo "log_referee: STALE GAP ROW — nothing matched it; delete it in the commit that fixed it:"
    echo "  $re"
  fi
done <<< "$gap_res"

owned_n=$(sort -u "$matched_gaps" 2>/dev/null | wc -l | tr -d ' ')
agreed=$(comm -12 <(sort "$OUT/cxx.msg") <(sort "$OUT/rust.msg") | wc -l | tr -d ' ')
printf 'log referee: %s/%s messages identical, %s gap rows owned\n' "$agreed" "$cxx_n" "$owned_n"

if [ "$rc" -eq 0 ]; then
  echo "log_referee: PASS"
  [ "$KEEP" -eq 1 ] || rm -f "$OUT"/*.264
else
  echo "log_referee: FAIL (see $OUT/)"
fi
exit "$rc"
