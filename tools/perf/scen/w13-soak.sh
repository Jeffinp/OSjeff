# W13 proof (d): open/close soak of WASM apps. 100 rounds of "Ola" (hello) and
# "Pintura" (paint, 1 MiB bitmap + two surfaces) opened from the Start panel and
# closed with the title-bar button. Build with --features perf-trace and compare
# the serial `[trace]   heap used=` lines (tools/perf/w8-heap.sh): the heap must
# not drift, which also proves the Store, surfaces and descriptors are freed.
# The Start panel has more rows than fit: `End` scrolls it to the bottom, where Ola is
# row 6 and Pintura row 7 (W18: the apps now come from the disk volume, so every open
# also reads /apps/<id>.wasm).
# Window outer sizes: hello 408x296, paint 528x400, both at (240,130): the close
# button is at (x + w - 18, 145).
source "$(dirname "$0")/../lib.sh"
srow() { echo $(( DOCKY - 529 + 38 * $1 )); }
wait_first_frame
ROUNDS=${ROUNDS:-100}
for r in $(seq 1 "$ROUNDS"); do
  dock 451; click; sleep 0.4; key end; sleep 0.3; goto 440 "$(srow 6)"; click; sleep 0.9
  goto 630 145; click; sleep 0.7
  dock 451; click; sleep 0.4; key end; sleep 0.3; goto 440 "$(srow 7)"; click; sleep 0.9
  goto 750 145; click; sleep 0.7
done
sleep 3
shot soak-end
finish
