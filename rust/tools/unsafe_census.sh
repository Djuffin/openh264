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
# **`src/api/` is out of scope for the tracking number, but not for this pin —
# S12.11, and the reason is the whole of `api_island_scope.md`.** The exemption used
# to be the directory: every allow under `src/api/` was skipped, on the grounds that
# it is "the frozen C-ABI layer, watched by `abi_exports.sh` and `abi_sizes.sh`
# instead". Measured, that covered far more than the boundary — 193 of
# `codec_api.rs`'s 218 raw dereferences were in **non-`extern "C"`** bodies, decoder
# picture-reordering logic that had simply never been looked at, because the two
# referees that do watch the island watch its *surface* (seven exported symbols, 170
# type sizes) and neither reads a body.
#
# So the exemption is now the **ABI surface**, not the directory. An allow under
# `src/api/` is skipped only when the item it attaches to is `extern "C"`; every
# other one is pinned as an `island-nonextern(<cat>)` row and fails in both
# directions like the rest. They are not counted into the tracking number — that
# stays "allows outside `src/api/`", so the series remains comparable — but a *new*
# one cannot appear unannounced, which is exactly what happened for eleven sessions.
# An allow whose item cannot be classified is pinned rather than skipped.
#
# **`cfg-region(...)` rows — S12.5, F303.** Everything above counts what the host
# build compiles, and that is not the whole tree. `WelsEmms`'s `asm!("emms")` sat
# behind `#[cfg(target_arch = "x86_64")]` for eleven sessions with no
# `#[allow(unsafe_code)]`: on this aarch64 host the block is stripped before the
# lint runs, so `#![deny(unsafe_code)]` never fired, this census had no attribute
# to count, and an x86_64 build simply did not compile. No instrument here could
# have caught it, because every one of them measures one architecture.
#
# So the conditional regions themselves are pinned, over the **whole** of `src/`
# including `api/`: one row per `#[cfg(...)]` on a target, feature or `not(test)`
# predicate, keyed by the predicate text. They are not unsafe and are not the
# tracking number — they are the list of places where the other rows are only
# true for the host you measured on, and a new one has to be justified the same
# way a new allow does.
#
# **The pinned set is empty, and that is the intended state.** S12.5 deleted the
# `emms` rather than tagging it: upstream declares `WelsEmms` only under
# `#if defined(X86_ASM)`, this port translates none of the assembly kernels, and
# a crate that executes no MMX has nothing for `emms` to clear. So the list has
# no members and this check is a tripwire — the next `#[cfg(target_*)]` anyone
# adds fails it and has to be argued for.
#
# Two predicate families are excluded because the gates already compile both
# arms, so nothing is blind: `cfg(test)` (the test battery) and
# `debug_assertions` (every gate runs debug **and** release).
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
        island = rel.startswith('api/')
        lines = open(path).read().splitlines()
        for i, line in enumerate(lines):
            if line.strip() != '#[allow(unsafe_code)]':
                continue
            if island:
                # The ABI surface is the exemption, not the directory (S12.11): find
                # the item this allow attaches to and skip it only if it is
                # `extern "C"`. Anything else — and anything unclassifiable — pins.
                item = ''
                for c in lines[i + 1:i + 9]:
                    t = c.strip()
                    if not t or t.startswith('//') or t.startswith('#['):
                        continue
                    item = t
                    break
                if 'extern "C"' in item:
                    continue
            cat = None
            for c in lines[max(0, i - 4):i]:
                m = re.search(r'unsafe-cat:\s*([A-Za-z0-9_()-]+)', c)
                if m:
                    cat = m.group(1)
            if cat is None:
                untagged.append(f'{rel}:{i + 1}')
                cat = 'UNTAGGED'
            if island:
                cat = f'island-nonextern({cat})'
            rows[(rel, cat)] = rows.get((rel, cat), 0) + 1
# F303: the conditional regions the host build never compiles. Whole tree,
# `api/` included — the blindness is architectural, not layered.
cfgre = re.compile(r'#\[cfg\(([^\n]*)\)\]\s*$')
for root, _, files in os.walk(src):
    for f in sorted(files):
        if not f.endswith('.rs'):
            continue
        path = os.path.join(root, f)
        rel = os.path.relpath(path, src)
        for line in open(path).read().splitlines():
            t = line.strip()
            if t.startswith('//'):
                continue
            m = cfgre.match(t)
            if not m:
                continue
            pred = m.group(1)
            if 'test' in pred and 'not(' not in pred:
                continue
            if not re.search(r'target_|feature|not\(test', pred):
                continue
            key = 'cfg-region(' + re.sub(r'[\s"]', '', pred) + ')'
            rows[(rel, key)] = rows.get((rel, key), 0) + 1

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
      echo "#   island-nonextern(...)   inside src/api/ but NOT on an extern \"C\" item (S12.11)"
      echo "#   cfg-region(...)         a conditional region the host build never sees (F303)"
      echo "#"
      census
    } > "$PIN"
    echo "unsafe_census: wrote $(grep -vc '^#' "$PIN") rows to tools/unsafe_census.txt"
    ;;
  report)
    # The allow rows and the cfg-region rows are different quantities and are
    # totalled apart: TOTAL is the tracking number (allows outside src/api/), and
    # the cfg regions are a separate list of blind spots, not unsafe.
    census | awk '{ n = substr($3, 2)
                    if ($2 ~ /^cfg-region\(/)            { g[$2] += n; tg += n }
                    else if ($2 ~ /^island-nonextern\(/)  { s[$2] += n; ts += n }
                    else                                  { c[$2] += n; t  += n } }
                  END { for (k in c) printf "  %-34s %d\n", k, c[k]
                        printf "  %-34s %d\n", "TOTAL (allows outside api/)", t
                        for (k in s) printf "  %-34s %d\n", k, s[k]
                        printf "  %-34s %d\n", "TOTAL island non-extern", ts
                        for (k in g) printf "  %-34s %d\n", k, g[k]
                        printf "  %-34s %d\n", "TOTAL cfg-regions", tg }' \
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
