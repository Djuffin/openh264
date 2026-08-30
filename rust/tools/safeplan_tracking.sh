#!/bin/bash
# The plan's §9 tracking number, as a command instead of prose.
#
#   usage: rust/tools/safeplan_tracking.sh [git-ref]
#
# With no argument it counts the working tree; with a ref it counts that commit,
# so a session can check its own opening figure against the previous close
# instead of trusting the hand-off brief. F219: the brief said 614 where the tree
# said 613, and the commit that "corrected" it moved a right number to a wrong
# one — the figure is the plan's single progress metric, so it gets a command.
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
CRATE_REL="rust/crates/openh264-rs"

if [ $# -ge 1 ]; then
  SNAP=$(mktemp -d)
  trap 'rm -rf "$SNAP"' EXIT
  (cd "$ROOT" && git archive "$1" "$CRATE_REL/src") | tar -x -C "$SNAP"
  DIR="$SNAP/$CRATE_REL"
  WHERE="$1"
else
  DIR="$ROOT/$CRATE_REL"
  WHERE="working tree"
fi

cd "$DIR"

# **S8: the metric counted its own documentation.** The grep below used to run
# unfiltered, so every *comment* that mentions `#[allow(unsafe_code)]` scored as an
# allow. Seven did, six of them in `src/decoder/` — comments whose text says the
# decoder "carries **two** `#[allow(unsafe_code)]` items in total" and that a grep
# "reads **three** by grep". The tree had written down the exact defect the tool
# then committed. F219 built this command because the figure is the plan's single
# progress metric and a hand count had moved a right number to a wrong one; a
# command that counts prose is the same failure with a longer lifetime.
#
# An attribute line is one whose match is not behind a `//` or `//!`. Both figures
# are printed: `raw` is what every session before S8 recorded, so the plan's tables
# stay readable against the number that produced them.
attrs() { grep -rn '#\[allow(unsafe_code)\]' --include='*.rs' src/ \
            | grep -vE '^[^:]+:[0-9]+:[[:space:]]*//'; }
outside=$(attrs | grep -vc '^src/api/')
inside=$(attrs | grep -c '^src/api/')
raw_out=$(grep -rn '#\[allow(unsafe_code)\]' --include='*.rs' src/ | grep -vc '^src/api/')
echo "tracking (${WHERE}): #[allow(unsafe_code)] outside src/api/ = ${outside}   (api: ${inside})"
echo "  (raw grep, pre-S8 basis incl. comment mentions: ${raw_out} outside — the delta is documentation)"
