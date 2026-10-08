#!/usr/bin/env bash
# Rebuilds assets/fonts/Inter-{Regular,Medium,SemiBold}.subset.ttf from the upstream
# Inter 4.0 static TrueType files (SIL OFL 1.1, see assets/fonts/OFL.txt).
#
#   tools/subset-font.sh <dir with Inter-Regular.ttf Inter-Medium.ttf Inter-SemiBold.ttf>
#
# Needs fonttools (`pip install fonttools`). The subset keeps Basic Latin, Latin-1
# (Portuguese accents), common punctuation, arrows, keyboard symbols (command, option,
# control, shift) and a few geometric glyphs; the `kern` feature is kept, hinting and
# every other table the rasteriser does not read is dropped.
set -eu
src=${1:?usage: subset-font.sh <inter ttf dir>}
out=$(dirname "$0")/../assets/fonts
unicodes="U+0020-007E,U+00A0-00FF,U+0131,U+0152,U+0153,U+0160,U+0161,U+0178,U+017D,U+017E,U+0192,U+02C6,U+02DC,U+2013,U+2014,U+2018-201A,U+201C-201E,U+2020,U+2021,U+2022,U+2026,U+2030,U+2039,U+203A,U+20AC,U+2122,U+2190-2193,U+2196-2199,U+21E7,U+2212,U+2303,U+2318,U+2325,U+232B,U+2326,U+23CE,U+2713,U+2715,U+25B2,U+25B6,U+25BC,U+25C0,U+25CB,U+25CF,U+2190,U+00B7"
for w in Regular Medium SemiBold; do
  pyftsubset "$src/Inter-$w.ttf" --unicodes="$unicodes" --layout-features='kern' \
    --no-hinting --name-IDs=0,1,2,4,6,13,14 --notdef-outline --drop-tables+=DSIG,STAT,GDEF,GSUB \
    --output-file="$out/Inter-$w.subset.ttf"
done
ls -l "$out"
