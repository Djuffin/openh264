#!/bin/bash
# The external-ABI harness — plan §7.2 gate 7(iii), Phase 8 session C (T8.C5).
#
#   usage: rust/tools/abi_harness/run.sh
#
# Builds the Rust `cdylib`, builds `abi_harness` against upstream's own headers, and
# drives the library through `dlopen` + `dlsym` + the vtables — the way a drop-in
# consumer does, and the one way no in-process gate can.
#
#   part 1  decoder conformance: every asset `decoder_conformance_test.rs` pins, run
#           through the dylib, per-asset SHA-1 == the in-process golden. The list is
#           **extracted from that test file at run time**, so the two can never drift.
#   part 2  encode loopback: fourteen configurations spanning the diffharness presets,
#           encoded through the dylib and compared byte for byte with `rust_enc`'s
#           in-process output. Both build profiles, on both sides.
#   part 3  the version pair and the capability block against `codec_ver.h` and
#           `welsDecoderExt.cpp`.
#   part 4  `res/Error_I_P.264` — F77's stream — returns an error code through the
#           dylib with the process alive.
#   part 5  `DecodeFrameNoDelay` — the *other* decode entry point (F82), whose
#           emission timing differs from `DecodeFrame2`'s and which every other gate
#           in this project misses because they all drive `DecodeFrame2`.
#
# Prints one `TALLY` line; `gates.sh` parses it and corroborates the exit status
# against it.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
CRATE="$ROOT/rust/crates/openh264-rs"
DIFF="$ROOT/rust/tools/diffharness"
OUT="$HERE/out"
mkdir -p "$OUT"

PASS=0; FAIL=0
ok()   { PASS=$((PASS+1)); printf 'PASS  %s\n' "$1"; }
bad()  { FAIL=$((FAIL+1)); printf 'FAIL  %s\n' "$1"; }

# --- build the harness ------------------------------------------------------
# `-x c++` on a `.cpp` is redundant but explicit: this driver is compiled the way a
# consumer's own code is, from the same headers, by a different compiler than the one
# that built the library. That difference is the point.
if ! c++ -std=c++11 -I "$ROOT/codec/api/wels" -o "$HERE/abi_harness" "$HERE/abi_harness.cpp" 2> "$OUT/build.log"; then
  sed -n '1,20p' "$OUT/build.log"
  echo "TALLY harness build failed"
  exit 1
fi

# --- the goldens, from the test file itself ---------------------------------
# `asset_test!(name, "file", "hash");` -> `file hash 0`
# `asset_test_concealed!(...)`         -> `file hash 1`
awk '
/^asset_test!\(/           { c = 0 }
/^asset_test_concealed!\(/ { c = 1 }
/^asset_test(_concealed)?!\(/ {
  n = split($0, a, "\"")
  if (n >= 5) printf "%s %s %d\n", a[2], a[4], c
}' "$CRATE/tests/decoder_conformance_test.rs" > "$OUT/goldens.txt"
NGOLD=$(grep -c . "$OUT/goldens.txt")

# The F82 rows, from `tests/decoder_nodelay_parity_test.rs`'s own table -> `<asset>
# <sha1> <frames>`. Extracted at run time for the same reason the goldens are: the
# in-process test and the through-the-dylib check must not be able to drift, and both
# carry the C++ decoder's numbers rather than the port's.
awk '
/^        asset: "/ { split($0, a, "\""); asset = a[2] }
/^        frames: /  { fr = $2; sub(/,$/, "", fr) }
/^        sha1: "/  { split($0, b, "\""); printf "%s %s %s\n", asset, b[2], fr }
' "$CRATE/tests/decoder_nodelay_parity_test.rs" > "$OUT/nodelay.txt"
NNODELAY=$(grep -c . "$OUT/nodelay.txt")
if [ "$NNODELAY" -lt 3 ]; then
  echo "FAIL  only $NNODELAY nodelay rows extracted from decoder_nodelay_parity_test.rs — the extractor is broken"
  echo "TALLY nodelay extraction failed"
  exit 1
fi
if [ "$NGOLD" -lt 50 ]; then
  echo "FAIL  only $NGOLD goldens extracted from decoder_conformance_test.rs — the extractor is broken, which would make part 1 vacuous"
  echo "TALLY goldens extraction failed"
  exit 1
fi

export ABI_HARNESS_RES="$ROOT"

# --- the fourteen encode configurations -------------------------------------
# "<yuv> <w> <h> <frames> <qp> <cabac> <gop> [rc] [baseinit] [slicemode] [slicenum]
#  [threads] [complexity] [ltr] [ltrperiod] [ltrfb]" — cxx_enc's own argv, minus the
# output path, which this script supplies.
#
# Spanning the presets: both entropy coders, all five iRCMode values, all three init
# paths, all four slice modes, threads 1/2, both complexity families, and LTR
# feedback. **No configuration here is `sm=3` with `t` in {2,4}** — that is F3's
# signature (phase0_findings.md), a pre-existing race in slice-list growth, and a
# gate is not the place to sample a known flake.
CONFIGS=(
  "res/CiscoVT2people_160x96_6fps.yuv 160 96 16 26 0 0 -1"
  "res/CiscoVT2people_160x96_6fps.yuv 160 96 16 26 1 0 -1"
  "res/CiscoVT2people_320x192_12fps.yuv 320 192 16 26 1 4 0"
  "res/CiscoVT2people_320x192_12fps.yuv 320 192 16 30 0 0 1"
  "res/Static_152_100.yuv 152 100 16 26 0 0 2"
  "res/CiscoVT2people_160x96_6fps.yuv 160 96 16 26 0 0 3"
  "res/CiscoVT2people_160x96_6fps.yuv 160 96 16 26 0 0 -1 1"
  "res/CiscoVT2people_320x192_12fps.yuv 320 192 16 26 0 0 -1 2"
  "res/CiscoVT2people_320x192_12fps.yuv 320 192 16 26 1 0 -1 0 1 2"
  "res/CiscoVT2people_320x192_12fps.yuv 320 192 16 26 0 0 -1 0 2 3"
  "res/CiscoVT2people_320x192_12fps.yuv 320 192 16 26 1 0 -1 0 3 1500 1"
  "res/CiscoVT2people_160x96_6fps.yuv 160 96 16 26 0 0 -1 0 0 1 2"
  "res/CiscoVT2people_160x96_6fps.yuv 160 96 16 26 0 0 -1 0 0 1 1 1"
  "res/CiscoVT2people_160x96_6fps.yuv 160 96 16 26 0 0 -1 0 0 1 1 0 2 8 1"
)

cd "$ROOT" || exit 1

for PROFILE in debug release; do
  echo
  echo "=== profile: $PROFILE"

  if [ "$PROFILE" = release ]; then
    (cd "$CRATE" && cargo build --release --quiet) || { bad "cdylib build ($PROFILE)"; continue; }
    (cd "$DIFF/rust_enc" && cargo build --release --quiet) || { bad "rust_enc build ($PROFILE)"; continue; }
  else
    (cd "$CRATE" && cargo build --quiet) || { bad "cdylib build ($PROFILE)"; continue; }
    (cd "$DIFF/rust_enc" && cargo build --quiet) || { bad "rust_enc build ($PROFILE)"; continue; }
  fi
  LIB="$CRATE/target/$PROFILE/libopenh264_rs.dylib"
  [ -f "$LIB" ] || LIB="$CRATE/target/$PROFILE/libopenh264_rs.so"
  if [ ! -f "$LIB" ]; then bad "no cdylib at $CRATE/target/$PROFILE"; continue; fi
  export ABI_HARNESS_LIB="$LIB"
  RUST_ENC="$DIFF/rust_enc/target/$PROFILE/rust_enc"

  # -- part 0: the seven resolve, and the shared SHA-1 still agrees with Rust's
  if "$HERE/abi_harness" selftest > "$OUT/selftest_$PROFILE.log" 2>&1; then
    ok "selftest ($PROFILE): the seven resolved through dlsym, SHA-1 known-answer OK"
  else
    sed -n '1,10p' "$OUT/selftest_$PROFILE.log"; bad "selftest ($PROFILE)"
  fi

  # -- part 3: version + capability
  if "$HERE/abi_harness" version > "$OUT/version_$PROFILE.log" 2>&1; then
    ok "version + capability ($PROFILE): $(grep -o 'WelsGetCodecVersion   -> [0-9.]*' "$OUT/version_$PROFILE.log")"
  else
    sed -n '1,10p' "$OUT/version_$PROFILE.log"; bad "version + capability ($PROFILE)"
  fi

  # -- part 4: F77's stream, through the dylib
  if "$HERE/abi_harness" error Error_I_P.264 > "$OUT/error_$PROFILE.log" 2>&1; then
    ok "malformed stream ($PROFILE): $(grep -o '[0-9]* frames, state union 0x[0-9a-f]*, process alive' "$OUT/error_$PROFILE.log")"
  else
    sed -n '1,10p' "$OUT/error_$PROFILE.log"; bad "malformed stream ($PROFILE) — see $OUT/error_$PROFILE.log"
  fi

  # -- part 1: decoder conformance
  if "$HERE/abi_harness" conformance "$OUT/goldens.txt" > "$OUT/conformance_$PROFILE.log" 2>&1; then
    ok "conformance ($PROFILE): $(grep -o '[0-9]*/[0-9]* assets bit-identical to the in-process goldens' "$OUT/conformance_$PROFILE.log")"
  else
    grep '  FAIL' "$OUT/conformance_$PROFILE.log" | head -10
    bad "conformance ($PROFILE): $(grep -o '[0-9]*/[0-9]* assets' "$OUT/conformance_$PROFILE.log" | head -1) — see $OUT/conformance_$PROFILE.log"
  fi

  # -- part 5: DecodeFrameNoDelay, the other decode entry point (F82)
  if "$HERE/abi_harness" nodelay "$OUT/nodelay.txt" > "$OUT/nodelay_$PROFILE.log" 2>&1; then
    ok "nodelay ($PROFILE): $(grep -o '[0-9]*/[0-9]* assets match the reference'"'"'s rows' "$OUT/nodelay_$PROFILE.log")"
  else
    grep '  FAIL' "$OUT/nodelay_$PROFILE.log" | head -6
    bad "nodelay ($PROFILE) — see $OUT/nodelay_$PROFILE.log"
  fi

  # -- part 2: encode loopback
  enc_pass=0; enc_fail=0
  i=0
  for cfg in "${CONFIGS[@]}"; do
    i=$((i+1))
    tag=$(printf 'cfg%02d_%s' "$i" "$PROFILE")
    # The output path is argument **8**, between the seven mandatory arguments and
    # the optional tail — `compare.sh`'s calling convention, and both drivers parse
    # it positionally. Appending it after the tail instead put it in `iRCMode`'s slot
    # and made every configuration a different encode on the two sides; that is what
    # the first run of this harness reported, and it is why the split is explicit.
    #
    # Unquoted on purpose — this is bash, and the configuration line IS an argument
    # list. See sweep.sh's header for why this file is bash and not zsh.
    set -- $cfg
    head7="$1 $2 $3 $4 $5 $6 $7"
    shift 7
    tail_args="$*"
    "$HERE/abi_harness" enc $head7 "$OUT/dyl_$tag.264" $tail_args > "$OUT/dyl_$tag.log" 2>&1
    a_rc=$?
    "$RUST_ENC"                    $head7 "$OUT/inp_$tag.264" $tail_args > "$OUT/inp_$tag.log" 2>&1
    b_rc=$?
    if [ $a_rc -ne 0 ] || [ $b_rc -ne 0 ]; then
      echo "    cfg$i: driver exit dylib=$a_rc in-process=$b_rc — $cfg"
      enc_fail=$((enc_fail+1)); continue
    fi
    if cmp -s "$OUT/dyl_$tag.264" "$OUT/inp_$tag.264"; then
      enc_pass=$((enc_pass+1))
    else
      echo "    cfg$i DIFFERS: $cfg"
      cmp "$OUT/dyl_$tag.264" "$OUT/inp_$tag.264" 2>&1 | head -2 | sed 's/^/      /'
      enc_fail=$((enc_fail+1))
    fi
  done
  if [ "$enc_fail" -eq 0 ]; then
    ok "encode loopback ($PROFILE): $enc_pass/${#CONFIGS[@]} configurations byte-identical to rust_enc"
  else
    bad "encode loopback ($PROFILE): $enc_pass/${#CONFIGS[@]} byte-identical, $enc_fail differed"
  fi
done

echo
printf 'TALLY %d passed / %d failed  (%d conformance assets, %d nodelay rows, %d encode configs x 2 profiles)\n' \
  "$PASS" "$FAIL" "$NGOLD" "$NNODELAY" "${#CONFIGS[@]}"
[ "$FAIL" -eq 0 ] || exit 1
