#!/bin/sh
# Dump every Rust declaration of a type next to the C++ original, so the canonical
# copy can be chosen by diffing rather than by guessing.
#
# Usage: rust/tools/show_type.sh <TypeName> [CppTagName]
#   CppTagName defaults to Tag<TypeName-without-leading-S>, the OpenH264 convention
#   (SDqLayer -> TagDqLayer). Pass it explicitly when that guess is wrong.

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
T="$1"
TAG="${2:-Tag$(printf '%s' "$T" | sed 's/^[sS]//')}"

printf '=== C++: struct %s / %s ===\n' "$TAG" "$T"
grep -rn -A 60 "\(struct\|union\) $TAG\b" "$ROOT/codec" --include=*.h \
  | awk '/\} *[A-Za-z_]*;|\}[A-Za-z_ ]*, *\*/ { print; exit } { print }' \
  | head -80
printf '\n(no C++ hit above means the tag guess was wrong; pass it as $2)\n'

printf '\n=== Rust copies ===\n'
for f in "$ROOT"/rust/crates/openh264-rs/src/encoder/*.rs \
         "$ROOT"/rust/crates/openh264-rs/src/common/*.rs; do
  awk -v T="$T" -v F="$(basename "$f")" '
    $0 ~ "^(pub )?(struct|enum|union) " T "([ <{]|$)" { show=1; depth=0 }
    show {
      print F ": " $0
      n = gsub(/\{/, "{"); m = gsub(/\}/, "}"); depth += n - m
      if (depth <= 0 && /\}/) { show=0; print "" }
    }
  ' "$f"
done
