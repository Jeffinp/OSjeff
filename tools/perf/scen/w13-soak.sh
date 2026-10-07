# W13 proof (d): open/close soak of WASM apps. 100 rounds of "Ola" (hello) and
# "Pintura" (paint, 1 MiB bitmap + two surfaces) opened from the Start panel and
# closed with the title-bar button. Build with --features perf-trace and compare
# the serial `[trace]   heap used=` lines (tools/perf/w8-heap.sh): the heap must
# not drift, which also proves the Store, surfaces and descriptors are freed.
# Start-panel rows: Notas 7, Ola 8, Pintura 9, Plasma 10 (no scroll needed).
# Window outer sizes: hello 408x296, paint 528x400, both at (240,130): the close
# button is at (x + w - 18, 145).
source "$(dirname "$0")/../lib.sh"
srow() { echo $(( DOCKY - 529 + 38 * $1 )); }
wait_first_frame
ROUNDS=${ROUNDS:-100}
for r in $(seq 1 "$ROUNDS"); do
  dock 451; click; sleep 0.4; goto 440 "$(srow 8)"; click; sleep 0.9
  goto 630 145; click; sleep 0.7
  dock 451; click; sleep 0.4; goto 440 "$(srow 9)"; click; sleep 0.9
  goto 750 145; click; sleep 0.7
done
sleep 3
shot soak-end
finish
