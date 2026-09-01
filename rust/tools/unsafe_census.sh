#!/bin/bash
# The **pinned unsafe census** — plan step E2's instrument.
#
#   usage: rust/tools/unsafe_census.sh [check | generate | report]
#
#     check      (default) recount and fail if the tree does not match the pin
#     generate   (over)write rust/tools/unsafe_census.txt from the tree
#     report     print the census grouped by category, exit 0
#
# **Why this exists, and why it is not the ratchet.** `unsafe_ratchet.sh` counts
# `unsafe` *syntax* per file and refuses increases; it is a ratchet, so a checkpoint
# may trade one shape for another and it will say so numerically. This is the
# opposite instrument: an **equality test over an enumerated set**. Every remaining
# `#[allow(unsafe_code)]` outside `src/api/` is listed here by file, category and
# count, and a tree that does not match — a new allow, a retired one still pinned, a
# category quietly changed — fails. D-exit-4's floor is a *list*, not a number, and
# a number cannot tell you whether the thing that stayed is the thing that was
# ruled to stay.
#
# The category comes from the `// unsafe-cat:` tag within four lines above the
# attribute (the tree's own convention). An **untagged** allow outside `src/api/`
# is a failure in itself: the tag is where the reason lives, and an allow with no
# reason is the shape every session's clean-up pass exists to find.
#
# `src/api/` is out of scope here, as it is for the tracking number: it is the
# frozen C-ABI layer, watched by `abi_exports.sh` and `abi_sizes.sh` instead.
#
# Comments are not attributes (S8's defect, F219): a line whose match sits behind
# `//` is documentation and is not counted. This tool reuses that rule verbatim.
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
SRC="$ROOT/rust/crates/openh264-rs/src"
PIN="$HERE/unsafe_census.txt"
MODE="${1:-check}"

# Emit "<file> <category> x<count>" lines, sorted — the census's stable form.
census() {
  python3 - "$SRC" <<'PY'
import os, re, sys
src = sys.argv[1]
rows = {}
untagged = []
for root, _, files in os.walk(src):
    for f in sorted(files):
        if not f.endswith('.rs'):
            continue
        path = os.path.join(root, f)
        rel = os.path.relpath(path, src)
        if rel.startswith('api/'):
            continue
        lines = open(path).read().splitlines()
        for i, line in enumerate(lines):
            if line.strip() != '#[allow(unsafe_code)]':
                continue
            cat = None
            for c in lines[max(0, i - 4):i]:
                m = re.search(r'unsafe-cat:\s*([A-Za-z0-9_()-]+)', c)
                if m:
                    cat = m.group(1)
            if cat is None:
                untagged.append(f'{rel}:{i + 1}')
                cat = 'UNTAGGED'
            rows[(rel, cat)] = rows.get((rel, cat), 0) + 1
for (rel, cat), n in sorted(rows.items()):
    print(f'{rel} {cat} x{n}')
if untagged:
    print('# UNTAGGED:', ' '.join(untagged), file=sys.stderr)
PY
}

case "$MODE" in
  generate)
    {
      echo "# The pinned unsafe census — every #[allow(unsafe_code)] outside src/api/,"
      echo "# by file and category. Regenerate deliberately, never to make a gate pass:"
      echo "# a diff here is a change in the tree's unsafe posture and belongs in the"
      echo "# commit message that causes it."
      echo "#"
      echo "# Categories, and what each means (lib.rs's preamble is the long form):"
      echo "#   instrument(test)        a Miri/provenance probe; D-exit-4 rules these stay"
      echo "#   C-ABI / C-ABI(test)     the frozen FFI boundary reaching in"
      echo "#   fork-shared(S63)        the MT fork's shared-context reads"
      echo "#   recon-seam              D-mt-3's one seam (holds the last unsafe impl)"
      echo "#   port-raw(...)           named residue, each with its reason at the site"
      echo "#   SCREEN_CONTENT(dormant) Phase 10's lane"
      echo "#"
      census
    } > "$PIN"
    echo "unsafe_census: wrote $(grep -vc '^#' "$PIN") rows to tools/unsafe_census.txt"
    ;;
  report)
    census | awk '{ c[$2] += substr($3, 2); t += substr($3, 2) }
                  END { for (k in c) printf "  %-24s %d\n", k, c[k]; printf "  %-24s %d\n", "TOTAL", t }' \
           | sort -k2 -rn
    echo "--- by file"
    census | sed 's/^/  /'
    ;;
  check)
    have=$(census)
    want=$(grep -v '^#' "$PIN" | grep -v '^[[:space:]]*$' || true)
    if [ "$have" = "$want" ]; then
      n=$(printf '%s\n' "$have" | grep -c . || true)
      echo "unsafe_census: PASS — $n pinned rows match the tree"
      exit 0
    fi
    echo "unsafe_census: FAIL — the tree does not match tools/unsafe_census.txt"
    echo "--- pinned but absent (retired: regenerate, and say so in the commit)"
    comm -13 <(printf '%s\n' "$have") <(printf '%s\n' "$want") | sed 's/^/  /'
    echo "--- present but unpinned (NEW unsafe: justify it, then regenerate)"
    comm -23 <(printf '%s\n' "$have") <(printf '%s\n' "$want") | sed 's/^/  /'
    exit 1
    ;;
  *)
    echo "usage: $0 [check | generate | report]" >&2
    exit 2
    ;;
esac
