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
outside=$(grep -rn '#\[allow(unsafe_code)\]' --include='*.rs' src/ | grep -vc '^src/api/')
inside=$(grep -rn '#\[allow(unsafe_code)\]' --include='*.rs' src/ | grep -c '^src/api/')
echo "tracking (${WHERE}): #[allow(unsafe_code)] outside src/api/ = ${outside}   (api: ${inside})"
