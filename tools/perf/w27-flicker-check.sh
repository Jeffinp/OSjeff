#!/usr/bin/env bash
# w27-flicker-check.sh <outdir>: after tools/perf/scen/w27-flicker.sh (Tarefas open, then the Snake
# game on top of it), the part of the Tarefas window that Snake does not cover must keep the same
# colour in every frame. Exit 1 if it ever changes (the window blinked).
set -u
out=${1:?usage: w27-flicker-check.sh <outdir>}
first=""
status=0
for f in "$out"/f[0-9]*.png; do
  p=$(convert "$f" -format "%[pixel:p{1000,600}]" info:)
  [ -z "$first" ] && first=$p
  [ "$p" = "$first" ] || { echo "$(basename "$f"): $p differs from $first"; status=1; }
done
[ $status -eq 0 ] && echo "stable: $first in every frame"
exit $status
