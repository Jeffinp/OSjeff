#!/usr/bin/env bash
# ab.sh <label> <imgA> <imgB> <reps> <modes> <scenarios...>
# Interleaved wall-clock A/B: for each rep/mode/scenario run A then B.
# Results: runs/<label>/<A|B>_<mode>_<scenario>_<n>/
cd "$(dirname "$0")"
label=$1; A=$2; B=$3; reps=$4; modes=$5; shift 5
unset QEMU_EXTRA
mkdir -p "runs/$label"
for n in $(seq 1 "$reps"); do
  for mode in $modes; do
    for sc in "$@"; do
      ./run.sh "$A/osjeff-$mode.img" "$mode" "runs/$label/A_${mode}_${sc}_$n" 150 "scen/$sc.sh"
      ./run.sh "$B/osjeff-$mode.img" "$mode" "runs/$label/B_${mode}_${sc}_$n" 150 "scen/$sc.sh"
      echo "done rep $n $mode $sc"
    done
  done
done
echo ALLDONE > "runs/$label/ALLDONE"
