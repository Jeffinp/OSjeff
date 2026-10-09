#!/usr/bin/env bash
# w27-burst-check.sh <outdir> <prefix> [max_pixels]: the screenshots <prefix>1.png, <prefix>2.png, ...
# were taken 0.2 s apart (see `burst` in tools/perf/scen/w27-oracle.sh). A desktop whose windows
# do not change must not change at all: count the pixels that differ between consecutive frames
# (HUD, the clocks and the parked pointer masked). More than max_pixels (default 0) in any step is
# flicker: a window or a shadow that was absent for a frame. Exit 1 on flicker.
set -u
out=${1:?usage: w27-burst-check.sh <outdir> <prefix> [max_pixels]}
pre=${2:?prefix}
max=${3:-0}
status=0
h=$(identify -format %h "$out/${pre}1.png")
mask=(-fill black -draw "rectangle 1040,0 1280,90" -draw "rectangle 560,0 720,30"
      -draw "rectangle 1130,$((h - 62)) 1275,$((h - 5))" -draw "rectangle 1255,365 1280,420")
prev="$out/${pre}1.png"
convert "$prev" "${mask[@]}" "$out/.b0.png"
worst=0
for f in $(ls "$out/$pre"[0-9]*.png | sort -V | tail -n +2); do
  convert "$f" "${mask[@]}" "$out/.b1.png"
  ae=$(compare -metric AE "$out/.b0.png" "$out/.b1.png" null: 2>&1 || true); ae=${ae%% *}
  [ "$ae" -gt "$worst" ] && worst=$ae
  if [ "$ae" -gt "$max" ]; then echo "$(basename "$f"): $ae pixels differ from the previous frame"; status=1; fi
  mv "$out/.b1.png" "$out/.b0.png"
done
rm -f "$out/.b0.png" "$out/.b1.png"
echo "burst $pre: worst step $worst pixels (limit $max)"
exit $status
