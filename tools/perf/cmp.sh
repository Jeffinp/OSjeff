#!/usr/bin/env bash
# cmp.sh <dirA> <dirB> <bios|uefi>
# Pixel-exact comparison of the *.png screenshots two runs produced with the same
# scenario (see scen/shots.sh). Only the HUD block (top-right, 1040,0..1280,80:
# live numbers + a shadow strip that accumulates under it) and the clock digits
# are masked, because they change every run; the clock pill and its shadow ARE
# compared. Prints "differing pixels (AE) = 0" for identical screens.
a=$1; b=$2; mode=${3:-bios}
tmp=$(mktemp -d)
if [ "$mode" = uefi ]; then H=800; else H=720; fi
PY=$((H-34-16))
for f in "$a"/*.png; do
  n=$(basename "$f")
  [ -f "$b/$n" ] || { echo "$n: missing in B"; continue; }
  for side in a b; do
    src=$a; [ $side = b ] && src=$b
    convert "$src/$n" -fill black -draw "rectangle 1040,0 1280,80" -draw "rectangle 560,0 720,30" \
      -draw "rectangle 1152,$((PY+7)) 1252,$((PY+25))" "$tmp/m$side.png"
  done
  ae=$(compare -metric AE "$tmp/ma.png" "$tmp/mb.png" null: 2>&1)
  echo "$n: differing pixels (AE) = $ae"
done
rm -rf "$tmp"
