#!/usr/bin/env bash
# w27-oracle-check.sh <outdir>: compare every <outdir>/rest<N>.png (the screen the incremental
# compositor left) with <outdir>/clean<N>.png (the same state recomposed from scratch, Ctrl+Alt+R)
# and print the number of differing pixels per pair. The HUD, the panel clock, the bar's clock
# pill and the parked pointer are masked, plus the rectangles of <outdir>/masks.txt (`N x y w h`: windows that change by
# themselves between the two shots). Exit status 1 if any pair differs; a diff image
# diff<N>.png (differences in red) is kept for those.
set -u
out=${1:?usage: w27-oracle-check.sh <outdir>}
status=0
bad=0; total=0
for f in $(ls "$out"/rest[1-9]*.png | sort -V); do
  [ -f "$f" ] || continue
  n=$(basename "$f" .png); n=${n#rest}
  c="$out/clean$n.png"
  [ -f "$c" ] || { echo "rest$n.png: no clean$n.png"; status=1; continue; }
  h=$(identify -format %h "$f")
  draws=(-fill black -draw "rectangle 1040,0 1280,90" -draw "rectangle 560,0 720,30"
         -draw "rectangle 1130,$((h - 62)) 1275,$((h - 5))"
         -draw "rectangle 1255,365 1280,420")   # the parked pointer (w20-cursor-check covers it)
  if [ -f "$out/masks.txt" ]; then
    while read -r mn mx my mw mh; do
      [ "$mn" = "$n" ] || continue
      draws+=(-draw "rectangle $mx,$my $((mx + mw)),$((my + mh))")
    done < "$out/masks.txt"
  fi
  # Content that changes by itself: whatever differs between the two ground-truth shots (taken a
  # moment apart, see W27_VOLATILE in the scenario) is widened a little and ignored.
  if [ -f "$out/clean2_$n.png" ]; then
    third="$out/clean3_$n.png"; [ -f "$third" ] || third="$out/clean2_$n.png"
    convert "$c" "$out/clean2_$n.png" -compose difference -composite -colorspace Gray -threshold 0 \
      \( "$out/clean2_$n.png" "$third" -compose difference -composite -colorspace Gray -threshold 0 \) \
      -compose lighten -composite -morphology Dilate Square:25 -negate "$out/.vol.png"
    convert "$c" "${draws[@]}" "$out/.vol.png" -compose multiply -composite "$out/.m0.png"
    convert "$f" "${draws[@]}" "$out/.vol.png" -compose multiply -composite "$out/.m1.png"
  else
    convert "$c" "${draws[@]}" "$out/.m0.png"; convert "$f" "${draws[@]}" "$out/.m1.png"
  fi
  ae=$(compare -metric AE "$out/.m0.png" "$out/.m1.png" "$out/.d.png" 2>&1 || true)
  ae=${ae%% *}
  total=$((total + 1))
  # Live content (Tarefas' numbers and chart) differs a little between the shots whatever the
  # compositor does: pairs with a volatile mask tolerate W27_TOL pixels (default 3500; the bugs
  # this oracle was written for differ by 4 700 to 62 000).
  tol=0; [ -f "$out/clean2_$n.png" ] && tol=${W27_TOL:-3500}
  if [ "$ae" -le "$tol" ]; then
    echo "pair $n: differing_pixels=$ae"
    rm -f "$out/diff$n.png"
  else
    bad=$((bad + 1)); status=1
    echo "pair $n: differing_pixels=$ae"
    mv "$out/.d.png" "$out/diff$n.png"
  fi
done
rm -f "$out/.m0.png" "$out/.m1.png" "$out/.d.png" "$out/.vol.png"
echo "pairs=$total differing_pairs=$bad"
exit $status
