#!/usr/bin/env bash
# batch.sh <imgdir> <mode> <label> <reps> <icount:0|1> <scenario>...
# Runs each scenario <reps> times; results in runs/<label>/<mode>_<scenario>_<n>/
cd "$(dirname "$0")"
imgdir=$1; mode=$2; label=$3; reps=$4; ic=$5; shift 5
for sc in "$@"; do
  for n in $(seq 1 "$reps"); do
    out=runs/$label/${mode}_${sc}_$n
    mkdir -p runs/$label
    if [ "$ic" = 1 ]; then export QEMU_EXTRA="-icount shift=0"; else unset QEMU_EXTRA; fi
    ./run.sh "$imgdir/kitsune-$mode.img" "$mode" "$out" 150 "scen/$sc.sh"
    echo "done $out"
  done
done
