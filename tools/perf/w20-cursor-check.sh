#!/usr/bin/env bash
# w20-cursor-check.sh <outdir>: compare every <outdir>/rest<N>.png (cursor parked after the
# sweeps, no repaint) with <outdir>/clean<N>.png (same screen after a forced full repaint, so the
# ground truth), masking the HUD (top right) and the clock pill (bottom right). Prints the number of
# differing pixels per pair; exit status 1 if any is non-zero.
set -u
out=${1:?usage: w20-cursor-check.sh <outdir>}
status=0
mask() { # in out
  local h; h=$(identify -format %h "$1")
  convert "$1" -fill black -draw "rectangle 1040,0 1280,90" \
    -draw "rectangle 1130,$((h - 62)) 1275,$((h - 5))" "$2"
}
for f in "$out"/rest[1-9]*.png; do
  [ -f "$f" ] || continue
  n=$(basename "$f" .png); n=${n#rest}
  c="$out/clean$n.png"
  [ -f "$c" ] || { echo "rest$n.png: no clean$n.png"; status=1; continue; }
  mask "$c" "$out/.m0.png"; mask "$f" "$out/.m1.png"
  ae=$(compare -metric AE "$out/.m0.png" "$out/.m1.png" "$out/diff$n.png" 2>&1 || true)
  echo "rest$n vs clean$n: differing_pixels=$ae"
  [ "$ae" = 0 ] || status=1
done
exit $status
