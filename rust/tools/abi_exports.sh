#!/bin/bash
# The exported-symbol gate — plan §7.2 gate 7(i), Phase 8 session C (T8.C3).
#
#   usage: rust/tools/abi_exports.sh [profile]      profile: release (default) | debug
#
# Builds the crate's `cdylib` and asserts its dynamic export list is **exactly**
# upstream's seven names — no more, no fewer.
#
# Why a gate and not a review note: a Rust cdylib exports precisely its
# `#[no_mangle] pub extern` items, and that set is easy to grow by accident. At
# `98e53555`, the commit before this script existed, the list was **24**: the seven,
# plus fourteen `WelsSampleSad*_c` in `common/sad_common.rs` and three `WelsCabac*` in
# `encoder/set_mb_syn_cabac.rs`. Every one of those seventeen is *also a symbol
# `libopenh264` exports*, so a consumer that had both libraries loaded would have had
# one interpose on the other — a linkage defect that no in-process test can see,
# because the rlib the tests link has no dynamic symbol table at all.
#
# `nm` spellings differ by platform and both are handled:
#   macOS / Mach-O   nm -gU libfoo.dylib   -> "<addr> T _WelsCreateDecoder"
#   Linux / ELF      nm -D --defined-only libfoo.so
# ELF shared objects also define linker-generated names (`_init`, `_fini`,
# `_edata`, `_end`, `__bss_start`) that are not API; they are filtered by name.
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
CRATE="$ROOT/rust/crates/openh264-rs"
PROFILE=${1:-release}

case "$PROFILE" in
  release) CARGO_FLAGS=(--release); OUT="$CRATE/target/release" ;;
  debug)   CARGO_FLAGS=();          OUT="$CRATE/target/debug"   ;;
  *) echo "usage: $0 [release|debug]"; exit 2 ;;
esac

# The contract. `codec_api.h`'s seven `extern "C"` declarations, and nothing else.
EXPECTED="WelsCreateDecoder
WelsCreateSVCEncoder
WelsDestroyDecoder
WelsDestroySVCEncoder
WelsGetCodecVersion
WelsGetCodecVersionEx
WelsGetDecoderCapability"

echo "=== building the cdylib ($PROFILE)"
if ! (cd "$CRATE" && cargo build "${CARGO_FLAGS[@]}" 2>&1 | tail -3); then
  echo "FAIL: cargo build failed"
  exit 1
fi

LIB=""
for cand in "$OUT/libopenh264_rs.dylib" "$OUT/libopenh264_rs.so"; do
  [ -f "$cand" ] && LIB="$cand"
done
if [ -z "$LIB" ]; then
  echo "FAIL: no cdylib in $OUT — is crate-type = [\"rlib\", \"cdylib\", \"staticlib\"] still set?"
  exit 1
fi
echo "    $LIB"

case "$LIB" in
  *.dylib) RAW=$(nm -gU "$LIB") ;;
  *)       RAW=$(nm -D --defined-only "$LIB") ;;
esac

# Column 3 is the name for a defined symbol (`<addr> <type> <name>`); strip the
# Mach-O leading underscore, drop the ELF linker-generated names, sort.
ACTUAL=$(printf '%s\n' "$RAW" \
  | awk 'NF >= 3 { print $3 }' \
  | sed 's/^_//' \
  | grep -vxE '_init|_fini|_edata|_end|__bss_start|init|fini' \
  | sort -u)

WANT=$(printf '%s\n' "$EXPECTED" | sort -u)

n_actual=$(printf '%s\n' "$ACTUAL" | grep -c . )
n_want=$(printf '%s\n' "$WANT" | grep -c . )

printf '\n%-8s %s\n' "expected" "$n_want"
printf '%-8s %s\n' "exported" "$n_actual"

if [ "$ACTUAL" = "$WANT" ]; then
  printf '\nexports: %s/%s — exactly upstream'"'"'s seven\n' "$n_actual" "$n_want"
  printf 'OK\n'
  exit 0
fi

printf '\nthe cdylib does not export exactly upstream'"'"'s seven:\n'
comm -13 <(printf '%s\n' "$WANT") <(printf '%s\n' "$ACTUAL") | sed 's/^/  EXTRA    /'
comm -23 <(printf '%s\n' "$WANT") <(printf '%s\n' "$ACTUAL") | sed 's/^/  MISSING  /'
printf '\nAn EXTRA is an internal name that escaped: delete its #[no_mangle], do not\n'
printf 'add it here. A MISSING is a broken drop-in: the seven are the contract.\n'
printf 'FAIL\n'
exit 1
