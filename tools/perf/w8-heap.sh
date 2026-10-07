#!/usr/bin/env bash
# w8-heap.sh <serial.log> : print the exact heap occupancy samples of a perf-trace
# run (one `[trace]   heap used=<bytes>B` line per second) as first / min / max /
# last, to judge whether an open/close soak (scen/w8-soak.sh) leaks.
f=${1:?usage: w8-heap.sh <serial.log>}
grep -a 'heap used=' "$f" | sed 's/.*used=\([0-9]*\)B.*/\1/' | awk '
  NR == 1 { first = $1; min = $1; max = $1 }
  { if ($1 < min) min = $1; if ($1 > max) max = $1; last = $1; n++ }
  END { if (n) printf "samples=%d first=%d min=%d max=%d last=%d (last-first=%d B)\n", n, first, min, max, last, last - first; else print "no heap samples (build with --features perf-trace)" }'
