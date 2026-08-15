#!/bin/bash
# Referee the whole malformed corpus against the C++ decoder — every table,
# every row (Phase 5 session U, T5.U2).
#
#   usage: rust/tools/ecref/compare_all.sh [dump-dir]
#
# Builds the corpus dump if it is not already there, then runs `compare.sh` per
# table with `MALFORMED_DUMP_DIR` set, so the `hdr*.*`, `tail.*` and degenerate
# rows are refereed alongside the `trunc.*` ones rather than left pinned against
# the port's own previous output.
#
# Prints the per-table tallies and a corpus total. Exit 0 iff every row was
# refereed and no dump fault fired; row divergences are reported, not fatal —
# they are evidence, and which of them are expected is the log's business, not a
# script's.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
CRATE="$ROOT/rust/crates/openh264-rs"
DUMP=${1:-${MALFORMED_DUMP_DIR:-/tmp/openh264-corpus-dump}}

# stem -> base stream. `degenerate` has none: its entries are built from
# parameter sets lifted out of SarVui.264 and never form a prefix of anything.
TABLES="
BA_MW_D:res/BA_MW_D.264
BA_MW_D_IDR_LOST:res/BA_MW_D_IDR_LOST.264
BA_MW_D_P_LOST:res/BA_MW_D_P_LOST.264
CABA2_SVA_B:res/CABA2_SVA_B.264
CABA3_SVA_B:res/CABA3_SVA_B.264
Cisco_Men_whisper_640x320_CABAC_Bframe_9:res/Cisco_Men_whisper_640x320_CABAC_Bframe_9.264
QCIF_2P_I_allIPCM:res/QCIF_2P_I_allIPCM.264
SVA_NL1_B:res/SVA_NL1_B.264
SarVui:res/SarVui.264
Static:res/Static.264
degenerate:-
narrow_16x16:res/narrow_16x16.264
narrow_16x16_idr_lost:res/narrow_16x16_idr_lost.264
sps_subsetsps_bothVUI:res/sps_subsetsps_bothVUI.264
"

if [ ! -d "$DUMP" ]; then
  echo "=== dumping the corpus to $DUMP"
  (cd "$CRATE" && MALFORMED_DUMP_DIR="$DUMP" cargo test --test malformed_stream_parity 2>&1) | tail -2
fi

[ -x "$HERE/ecref" ] || bash "$HERE/build.sh"

t_out_a=0; t_out_d=0; t_code_a=0; t_code_d=0; faults=0
printf '\n%-42s %18s %18s\n' "table" "output a/d" "codes a/d"
printf -- '---------------------------------------------------------------------------------\n'
for entry in $TABLES; do
  stem=${entry%%:*}; stream=${entry#*:}
  line=$(MALFORMED_DUMP_DIR="$DUMP" bash "$HERE/compare.sh" "$stream" "$stem.txt" 2>/dev/null \
           | tee "/tmp/ecref_$stem.log" | tail -1)
  rc=$?
  a=$(printf '%s' "$line" | sed -E 's/.*output ([0-9]+) agree.*/\1/')
  d=$(printf '%s' "$line" | sed -E 's/.*output [0-9]+ agree \/ ([0-9]+) differ.*/\1/')
  ca=$(printf '%s' "$line" | sed -E 's/.*codes ([0-9]+) agree.*/\1/')
  cd=$(printf '%s' "$line" | sed -E 's/.*codes [0-9]+ agree \/ ([0-9]+) differ.*/\1/')
  printf '%-42s %8s / %-7s %8s / %-7s%s\n' "$stem" "$a" "$d" "$ca" "$cd" \
    "$(printf '%s' "$line" | grep -q 'DUMP FAULTS' && printf '  *** DUMP FAULTS')"
  printf '%s' "$line" | grep -q 'DUMP FAULTS' && faults=$((faults+1))
  t_out_a=$((t_out_a+a)); t_out_d=$((t_out_d+d)); t_code_a=$((t_code_a+ca)); t_code_d=$((t_code_d+cd))
done
printf -- '---------------------------------------------------------------------------------\n'
printf '%-42s %8s / %-7s %8s / %-7s\n' "CORPUS" "$t_out_a" "$t_out_d" "$t_code_a" "$t_code_d"
printf 'rows refereed: %d   (per-table logs in /tmp/ecref_<stem>.log)\n' "$((t_out_a + t_out_d))"
[ "$faults" -eq 0 ] || { printf 'DUMP FAULTS in %d tables — the dump disagrees with the tables it referees\n' "$faults"; exit 1; }
